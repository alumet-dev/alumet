use anyhow::Context;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc, Mutex,
};
use tokio::{
    runtime,
    sync::Notify,
    task::{JoinError, JoinSet},
};

use crate::metrics::online::MetricReader;
use crate::pipeline::control::matching::OutputMatcher;
use crate::pipeline::elements::output::{run::run_async_output, AsyncOutputStream};
use crate::pipeline::matching::OutputNamePattern;
use crate::pipeline::naming::{namespace::Namespace2, OutputName};
use crate::pipeline::util::{
    channel,
    stream::{ControlledStream, SharedStreamState, StreamState},
};
use crate::{measurement::MeasurementBuffer, pipeline::error::PipelineError};

use super::{
    builder::{self, OutputBuilder},
    run::run_blocking_output,
};

/// A control messages for outputs.
#[derive(Debug)]
pub struct ControlMessage {
    /// Which output(s) to reconfigure.
    pub matcher: OutputMatcher,
    /// The new state to apply to the selected output(s).
    pub new_state: TaskState,
}

/// State of a (managed) output task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
#[repr(u8)]
pub enum TaskState {
    Run,
    Pause,
    StopNow,
    StopFinish,
}

impl From<u8> for TaskState {
    fn from(value: u8) -> Self {
        const RUN: u8 = TaskState::Run as u8;
        const PAUSE: u8 = TaskState::Pause as u8;
        const STOP_FINISH: u8 = TaskState::StopFinish as u8;

        match value {
            RUN => TaskState::Run,
            PAUSE => TaskState::Pause,
            STOP_FINISH => TaskState::StopFinish,
            _ => TaskState::StopNow,
        }
    }
}

pub enum SingleOutputController {
    Blocking(Arc<SharedOutputConfig>),
    Async(Arc<SharedStreamState>),
}

pub struct SharedOutputConfig {
    pub change_notifier: Notify,
    pub atomic_state: AtomicU8,
}

impl SharedOutputConfig {
    pub fn new() -> Self {
        Self {
            change_notifier: Notify::new(),
            atomic_state: AtomicU8::new(TaskState::Run as u8),
        }
    }

    pub fn set_state(&self, state: TaskState) {
        self.atomic_state.store(state as u8, Ordering::Relaxed);
        self.change_notifier.notify_one();
    }
}

impl SingleOutputController {
    pub fn set_state(&mut self, state: TaskState) {
        match self {
            SingleOutputController::Blocking(shared) => shared.set_state(state),
            SingleOutputController::Async(arc) => arc.set(StreamState::from(state as u8)),
        }
    }
}

pub(crate) struct OutputControl {
    tasks: TaskManager,
    /// Read-only access to the metrics.
    metrics: MetricReader,
}

struct TaskManager {
    spawned_tasks: JoinSet<Result<(), PipelineError>>,
    controllers: Vec<(OutputName, SingleOutputController)>,

    rx_provider: channel::ReceiverProvider,

    /// Handle of the "normal" async runtime. Used for creating new outputs.
    rt_normal: runtime::Handle,

    metrics: MetricReader,
}

impl OutputControl {
    pub fn new(rx_provider: channel::ReceiverProvider, rt_normal: runtime::Handle, metrics: MetricReader) -> Self {
        Self {
            tasks: TaskManager {
                spawned_tasks: JoinSet::new(),
                controllers: Vec::new(),
                rx_provider,
                rt_normal,
                metrics: metrics.clone(),
            },
            metrics,
        }
    }

    pub fn blocking_create_outputs(&mut self, outputs: Namespace2<OutputBuilder>) -> anyhow::Result<()> {
        let metrics = self.metrics.blocking_read();
        for ((plugin, output_name), builder) in outputs {
            let mut ctx = builder::OutputBuildContext {
                metrics: &metrics,
                metrics_r: &self.metrics.clone(),
                runtime: self.tasks.rt_normal.clone(),
            };
            let full_name = OutputName::new(plugin.clone(), output_name);
            self.tasks
                .create_output(&mut ctx, full_name, builder)
                .inspect_err(|e| log::error!("Error in output creation requested by plugin {plugin}: {e:#}"))?;
        }
        Ok(())
    }

    #[allow(unused)]
    pub async fn create_output(&mut self, name: OutputName, builder: builder::SendOutputBuilder) {
        let metrics = self.metrics.read().await;
        let mut ctx = builder::OutputBuildContext {
            metrics: &metrics,
            metrics_r: &self.metrics,
            runtime: self.tasks.rt_normal.clone(),
        };
        self.tasks.create_output(&mut ctx, name, builder.into());
    }

    pub fn handle_message(&mut self, msg: ControlMessage) -> anyhow::Result<()> {
        self.tasks.reconfigure(msg);
        Ok(())
    }

    pub async fn join_next_task(&mut self) -> Result<Result<(), PipelineError>, JoinError> {
        match self.tasks.spawned_tasks.join_next().await {
            Some(res) => res,
            None => unreachable!("join_next_task must be guarded by has_task to prevent an infinite loop"),
        }
    }

    pub fn has_task(&self) -> bool {
        !self.tasks.spawned_tasks.is_empty()
    }

    pub async fn shutdown<F>(mut self, handle_task_result: F)
    where
        F: FnMut(Result<Result<(), PipelineError>, tokio::task::JoinError>),
    {
        // Outputs naturally close when the input channel is closed,
        // but that only works when the output is running.
        // If the output is paused, it needs to be stopped with a command.
        let stop_msg = ControlMessage {
            matcher: OutputMatcher::Name(OutputNamePattern::wildcard()),
            new_state: TaskState::StopFinish,
        };
        self.handle_message(stop_msg)
            .expect("handle_message in shutdown should not fail");

        // Close the channel and wait for all outputs to finish
        self.tasks.shutdown(handle_task_result).await;
    }
}

impl TaskManager {
    fn create_output<'a>(
        &mut self,
        ctx: &'a mut builder::OutputBuildContext<'a>,
        name: OutputName,
        builder: OutputBuilder,
    ) -> anyhow::Result<()> {
        match builder {
            OutputBuilder::Blocking(builder) => self.create_blocking_output(ctx, name, builder),
            OutputBuilder::Async(builder) => self.create_async_output(ctx, name, builder),
        }
    }

    fn create_blocking_output(
        &mut self,
        ctx: &mut dyn builder::BlockingOutputBuildContext,
        name: OutputName,
        builder: Box<dyn builder::BlockingOutputBuilder>,
    ) -> anyhow::Result<()> {
        // Build the output.
        let output = builder(ctx).context("output creation failed")?;

        // Create the necessary context.
        let rx = self.rx_provider.get(); // to receive measurements
        let metrics = self.metrics.clone(); // to read metric definitions

        // Create and store the task controller.
        let config = Arc::new(SharedOutputConfig::new());
        let shared_config = config.clone();
        let control = SingleOutputController::Blocking(config);
        self.controllers.push((name.clone(), control));

        // Put the output in a Mutex to overcome the lack of tokio::spawn_scoped.
        let guarded_output = Arc::new(Mutex::new(output));

        // Spawn the task on the runtime.
        match rx {
            // Specialize on the kind of receiver at compile-time (for performance).
            channel::ReceiverEnum::Broadcast(rx) => {
                let task = run_blocking_output(name, guarded_output, rx, metrics, shared_config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
            channel::ReceiverEnum::Single(rx) => {
                let task = run_blocking_output(name, guarded_output, rx, metrics, shared_config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
        }

        Ok(())
    }

    fn create_async_output(
        &mut self,
        ctx: &mut dyn builder::AsyncOutputBuildContext,
        name: OutputName,
        builder: Box<dyn builder::AsyncOutputBuilder>,
    ) -> anyhow::Result<()> {
        use channel::MeasurementReceiver;

        fn box_controlled_stream<
            S: futures::Stream<Item = Result<MeasurementBuffer, channel::StreamRecvError>> + Send + 'static,
        >(
            stream: S,
        ) -> (AsyncOutputStream, Arc<SharedStreamState>) {
            let stream = Box::pin(ControlledStream::new(stream));
            let state = stream.state();
            (AsyncOutputStream(stream), state)
        }

        // For async outputs, we need to build the stream first
        let rx = self.rx_provider.get();
        let (stream, state) = match rx {
            channel::ReceiverEnum::Broadcast(receiver) => box_controlled_stream(receiver.into_stream()),
            channel::ReceiverEnum::Single(receiver) => box_controlled_stream(receiver.into_stream()),
        };

        // Create the output
        let output = builder(ctx, stream).context("output creation failed")?;

        // Create and store the task controller
        let control = SingleOutputController::Async(state);
        self.controllers.push((name.clone(), control));

        // Spawn the output
        let task = run_async_output(name, output);
        self.spawned_tasks.spawn_on(task, &self.rt_normal);
        Ok(())
    }

    fn reconfigure(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.controllers {
            if msg.matcher.matches(name) {
                output_config.set_state(msg.new_state);
            }
        }
    }

    async fn shutdown<F>(self, mut handle_task_result: F)
    where
        F: FnMut(Result<Result<(), PipelineError>, tokio::task::JoinError>),
    {
        // Drop the rx_provider first in order to close the channel.
        drop(self.rx_provider);
        let mut spawned_tasks = self.spawned_tasks;

        // Wait for all outputs to finish
        loop {
            match spawned_tasks.join_next().await {
                Some(res) => handle_task_result(res),
                None => break,
            }
        }
    }
}
