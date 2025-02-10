//! Implementation and control of output tasks.

use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use control_state::SingleOutputController;
use futures::Stream;
use tokio::runtime;
use tokio::task::{JoinError, JoinSet};

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::MetricReader;
use crate::metrics::registry::MetricRegistry;
use crate::pipeline::naming::matching::OutputMatcher;
use crate::pipeline::naming::namespace::Namespaces;
use crate::pipeline::naming::OutputName;
use crate::pipeline::util::channel::{self, RecvError};
use crate::pipeline::util::stream::{ControlledStream, SharedStreamState};

use super::error::WriteError;

pub mod builder;
mod control_state;

use builder::{
    AsyncOutputBuildContext, AsyncOutputBuilder, BlockingOutputBuildContext, BlockingOutputBuilder, OutputBuildContext,
    OutputBuilder,
};

/// A blocking output that exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

/// An asynchronous stream of measurements, to be used by an asynchronous output.
pub struct AsyncOutputStream(pub Pin<Box<dyn Stream<Item = Result<MeasurementBuffer, StreamRecvError>> + Send>>); // TODO make opaque?

pub type StreamRecvError = channel::StreamRecvError;
pub type BoxedAsyncOutput = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Shared data that can be accessed by outputs.
pub struct OutputContext<'a> {
    pub metrics: &'a MetricRegistry,
}

pub(crate) struct OutputControl {
    tasks: TaskManager,
    /// Read-only access to the metrics.
    metrics: MetricReader,
}

struct TaskManager {
    spawned_tasks: JoinSet<anyhow::Result<()>>,
    controllers: Vec<(OutputName, control_state::SingleOutputController)>,

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

    pub fn blocking_create_outputs(&mut self, outputs: Namespaces<OutputBuilder>) -> anyhow::Result<()> {
        let metrics = self.metrics.blocking_read();
        for ((plugin, output_name), builder) in outputs {
            let mut ctx = OutputBuildContext {
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
        let mut ctx = OutputBuildContext {
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

    pub fn has_task(&self) -> bool {
        !self.tasks.spawned_tasks.is_empty()
    }

    pub async fn join_next_task(&mut self) -> Result<anyhow::Result<()>, JoinError> {
        self.tasks
            .spawned_tasks
            .join_next()
            .await
            .expect("should not be called when !has_task()")
    }

    pub async fn shutdown<F>(mut self, handle_task_result: F)
    where
        F: Fn(Result<anyhow::Result<()>, tokio::task::JoinError>),
    {
        // Outputs naturally close when the input channel is closed,
        // but that only works when the output is running.
        // If the output is paused, it needs to be stopped with a command.
        let stop_msg = ControlMessage {
            matcher: OutputMatcher::wildcard(),
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
        ctx: &'a mut OutputBuildContext<'a>,
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
        ctx: &mut dyn BlockingOutputBuildContext,
        name: OutputName,
        builder: Box<dyn BlockingOutputBuilder>,
    ) -> anyhow::Result<()> {
        // Build the output.
        let output = builder(ctx).context("output creation failed")?;

        // Create the necessary context.
        let rx = self.rx_provider.get(); // to receive measurements
        let metrics = self.metrics.clone(); // to read metric definitions

        // Create and store the task controller.
        let config = Arc::new(control_state::SharedOutputConfig::new());
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
        ctx: &mut dyn AsyncOutputBuildContext,
        name: OutputName,
        builder: Box<dyn AsyncOutputBuilder>,
    ) -> anyhow::Result<()> {
        use channel::MeasurementReceiver;

        fn box_controlled_stream<
            S: Stream<Item = Result<MeasurementBuffer, channel::StreamRecvError>> + Send + 'static,
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

    async fn shutdown<F>(self, handle_task_result: F)
    where
        F: Fn(Result<anyhow::Result<()>, tokio::task::JoinError>),
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

async fn run_blocking_output<Rx: channel::MeasurementReceiver>(
    name: OutputName,
    guarded_output: Arc<Mutex<Box<dyn Output>>>,
    mut rx: Rx,
    metrics_reader: MetricReader,
    config: Arc<control_state::SharedOutputConfig>,
) -> anyhow::Result<()> {
    /// If `measurements` is an `Ok`, build an [`OutputContext`] and call `output.write(&measurements, &ctx)`.
    /// Otherwise, handle the error.
    async fn write_measurements(
        name: &OutputName,
        output: Arc<Mutex<Box<dyn Output>>>,
        metrics_r: MetricReader,
        maybe_measurements: Result<MeasurementBuffer, channel::RecvError>,
    ) -> anyhow::Result<ControlFlow<()>> {
        match maybe_measurements {
            Ok(measurements) => {
                log::trace!("writing {} measurements to {name}", measurements.len());
                let res = tokio::task::spawn_blocking(move || {
                    let ctx = OutputContext {
                        metrics: &metrics_r.blocking_read(),
                    };
                    output.lock().unwrap().write(&measurements, &ctx)
                })
                .await?;
                match res {
                    Ok(()) => Ok(ControlFlow::Continue(())),
                    Err(WriteError::CanRetry(e)) => {
                        log::error!("Non-fatal error when writing to {name} (will retry): {e:#}");
                        Ok(ControlFlow::Continue(()))
                    }
                    Err(WriteError::Fatal(e)) => {
                        log::error!("Fatal error when writing to {name} (will stop running): {e:?}");
                        Err(e.context(format!("fatal error when writing to {name}")))
                    }
                }
            }
            Err(channel::RecvError::Lagged(n)) => {
                log::warn!("Output {name} is too slow, it lost the oldest {n} messages.");
                Ok(ControlFlow::Continue(()))
            }
            Err(channel::RecvError::Closed) => {
                log::debug!("The channel connected to output {name} was closed, it will now stop.");
                Ok(ControlFlow::Break(()))
            }
        }
    }

    let config_change = &config.change_notifier;
    let mut receive = true;
    let mut finish = false;
    loop {
        tokio::select! {
            _ = config_change.notified() => {
                let new_state = config.atomic_state.load(Ordering::Relaxed);
                match new_state.into() {
                    TaskState::Run => {
                        receive = true;
                    }
                    TaskState::Pause => {
                        receive = false;
                    }
                    TaskState::StopNow => {
                        break; // stop the output and ignore the remaining data
                    }
                    TaskState::StopFinish => {
                        finish = true;
                        break; // stop the output and empty the channel
                    }
                }
            },
            measurements = rx.recv(), if receive => {
                let res = write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), measurements).await?;
                if res.is_break() {
                    finish = false; // just in case
                    break
                }
            }
        }
    }

    if finish {
        // Write the last measurements, ignore any lag (the latter is done in write_measurements).
        // This is useful when Alumet is stopped, to ensure that we don't discard any data.
        loop {
            log::trace!("{name} finishing...");
            let received = rx.recv().await;
            log::trace!(
                "{name} finishing with {}",
                match &received {
                    Ok(buf) => format!("Ok(buf of size {})", buf.len()),
                    Err(RecvError::Closed) => String::from("Err(Closed)"),
                    Err(RecvError::Lagged(n)) => format!("Err(Lagged({n}))"),
                }
            );
            let res = write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), received).await?;
            if res.is_break() {
                break;
            }
        }
    }

    Ok(())
}

async fn run_async_output(name: OutputName, output: BoxedAsyncOutput) -> anyhow::Result<()> {
    output.await.map_err(|e| {
        log::error!("Error when asynchronously writing to {name} (will stop running): {e:?}");
        e.context(format!("error in async output {name}"))
    })
}
