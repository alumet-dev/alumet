//! Implementation and control of output tasks.

use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use builder::{
    AsyncOutputBuildContext, AsyncOutputBuilder, BlockingOutputBuildContext, BlockingOutputBuilder, OutputBuildContext,
    OutputBuilder,
};
use control_state::SingleOutputController;
use futures::Stream;
use tokio::runtime;
use tokio::task::{JoinError, JoinSet};

use crate::measurement::MeasurementBuffer;
use crate::metrics::MetricRegistry;
use crate::pipeline::util::channel::{self, RecvError};
use crate::pipeline::util::matching::OutputSelector;
use crate::pipeline::util::naming::{NameGenerator, OutputName};
use crate::pipeline::util::stream::{ControlledStream, SharedStreamState};
use crate::pipeline::PluginName;

use super::super::registry;
use super::error::WriteError;

/// A blocking output that exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

/// An asynchronous stream of measurements, to be used by an asynchronous output.
pub struct AsyncOutputStream(
    pub Pin<Box<dyn Stream<Item = Result<MeasurementBuffer, channel::StreamRecvError>> + Send>>,
); // TODO make opaque?

pub type BoxedAsyncOutput = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Shared data that can be accessed by outputs.
pub struct OutputContext<'a> {
    pub metrics: &'a MetricRegistry,
}

pub(crate) struct OutputControl {
    tasks: TaskManager,
    names: NameGenerator,
    /// Read-only access to the metrics.
    metrics: registry::MetricReader,
}

struct TaskManager {
    spawned_tasks: JoinSet<anyhow::Result<()>>,
    controllers: Vec<(OutputName, control_state::SingleOutputController)>,

    rx_provider: channel::ReceiverProvider,

    /// Handle of the "normal" async runtime. Used for creating new outputs.
    rt_normal: runtime::Handle,

    metrics: registry::MetricReader,
}

impl OutputControl {
    pub fn new(
        rx_provider: channel::ReceiverProvider,
        rt_normal: runtime::Handle,
        metrics: registry::MetricReader,
    ) -> Self {
        Self {
            tasks: TaskManager {
                spawned_tasks: JoinSet::new(),
                controllers: Vec::new(),
                rx_provider,
                rt_normal,
                metrics: metrics.clone(),
            },
            names: NameGenerator::new(),
            metrics,
        }
    }

    pub fn blocking_create_outputs(&mut self, outputs: Vec<(PluginName, OutputBuilder)>) -> anyhow::Result<()> {
        let metrics = self.metrics.blocking_read();
        for (plugin, builder) in outputs {
            let mut ctx = OutputBuildContext {
                metrics: &metrics,
                namegen: self.names.plugin_namespace(&plugin),
                runtime: self.tasks.rt_normal.clone(),
            };
            self.tasks
                .create_output(&mut ctx, builder)
                .inspect_err(|e| log::error!("Error in output creation requested by plugin {plugin}: {e:#}"))?;
        }
        Ok(())
    }

    #[allow(unused)]
    pub async fn create_output(&mut self, plugin: PluginName, builder: builder::SendOutputBuilder) {
        let metrics = self.metrics.read().await;
        let mut ctx = OutputBuildContext {
            metrics: &metrics,
            namegen: self.names.plugin_namespace(&plugin),
            runtime: self.tasks.rt_normal.clone(),
        };
        self.tasks.create_output(&mut ctx, builder.into());
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
            selector: OutputSelector::all(),
            new_state: TaskState::StopFinish,
        };
        self.handle_message(stop_msg)
            .expect("handle_message in shutdown should not fail");

        // Wait for all outputs to finish
        loop {
            match self.tasks.spawned_tasks.join_next().await {
                Some(res) => handle_task_result(res),
                None => break,
            }
        }
    }
}

impl TaskManager {
    fn create_output<'a>(&mut self, ctx: &'a mut OutputBuildContext<'a>, builder: OutputBuilder) -> anyhow::Result<()> {
        match builder {
            OutputBuilder::Blocking(builder) => self.create_blocking_output(ctx, builder),
            OutputBuilder::Async(builder) => self.create_async_output(ctx, builder),
        }
    }

    fn create_blocking_output(
        &mut self,
        ctx: &mut dyn BlockingOutputBuildContext,
        builder: Box<dyn BlockingOutputBuilder>,
    ) -> anyhow::Result<()> {
        // Build the output.
        let reg = builder(ctx).context("output creation failed")?;

        // Create the necessary context.
        let rx = self.rx_provider.get(); // to receive measurements
        let metrics = self.metrics.clone(); // to read metric definitions

        // Create and store the task controller.
        let config = Arc::new(control_state::SharedOutputConfig::new());
        let shared_config = config.clone();
        let control = SingleOutputController::Blocking(config);
        self.controllers.push((reg.name.clone(), control));

        // Put the output in a Mutex to overcome the lack of tokio::spawn_scoped.
        let guarded_output = Arc::new(Mutex::new(reg.output));

        // Spawn the task on the runtime.
        match rx {
            // Specialize on the kind of receiver at compile-time (for performance).
            channel::ReceiverEnum::Broadcast(rx) => {
                let task = run_blocking_output(reg.name, guarded_output, rx, metrics, shared_config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
            channel::ReceiverEnum::Single(rx) => {
                let task = run_blocking_output(reg.name, guarded_output, rx, metrics, shared_config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
        }

        Ok(())
    }

    fn create_async_output(
        &mut self,
        ctx: &mut dyn AsyncOutputBuildContext,
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
        let reg = builder(ctx, stream).context("output creation failed")?;

        // Create and store the task controller
        let control = SingleOutputController::Async(state);
        self.controllers.push((reg.name.clone(), control));

        // Spawn the output
        let task = run_async_output(reg.name, reg.output);
        self.spawned_tasks.spawn_on(task, &self.rt_normal);
        Ok(())
    }

    fn reconfigure(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.controllers {
            if msg.selector.matches(name) {
                output_config.set_state(msg.new_state);
            }
        }
    }
}

/// A control messages for outputs.
#[derive(Debug)]
pub struct ControlMessage {
    /// Which output(s) to reconfigure.
    pub selector: OutputSelector,
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
    metrics_reader: registry::MetricReader,
    config: Arc<control_state::SharedOutputConfig>,
) -> anyhow::Result<()> {
    /// If `measurements` is an `Ok`, build an [`OutputContext`] and call `output.write(&measurements, &ctx)`.
    /// Otherwise, handle the error.
    async fn write_measurements(
        name: &OutputName,
        output: Arc<Mutex<Box<dyn Output>>>,
        metrics_r: registry::MetricReader,
        maybe_measurements: Result<MeasurementBuffer, channel::RecvError>,
    ) -> anyhow::Result<ControlFlow<()>> {
        match maybe_measurements {
            Ok(measurements) => {
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
    let mut finish = true;
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
                        finish = false;
                        break; // stop the output and ignore the remaining data
                    }
                    TaskState::StopFinish => {
                        break; // stop the output and empty the channel
                    }
                }
            },
            measurements = rx.recv(), if receive => {
                let res = write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), measurements).await?;
                if res.is_break() {
                    break
                }
            }
        }
    }

    if finish {
        // Write the last measurements, ignore any lag.
        // This is useful when Alumet is stopped, to ensure that we don't discard any data.
        loop {
            match rx.recv().await {
                res @ (Ok(_) | Err(RecvError::Lagged(_))) => {
                    write_measurements(&name, guarded_output.clone(), metrics_reader.clone(), res).await?;
                }
                Err(RecvError::Closed) => break,
            }
        }
    }

    Ok(())
}

async fn run_async_output(name: OutputName, output: BoxedAsyncOutput) -> anyhow::Result<()> {
    output.await.with_context(|| format!("error in async output {}", name))
}

mod control_state {
    use std::sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    };
    use tokio::sync::Notify;

    use super::TaskState;
    use crate::pipeline::util::stream::{SharedStreamState, StreamState};

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
}

pub mod builder {
    use tokio::runtime;

    use crate::{
        metrics::MetricRegistry,
        pipeline::util::naming::{OutputName, PluginElementNamespace},
    };

    use super::AsyncOutputStream;

    /// An output builder, for any type of output.
    ///
    /// Use this type in the pipeline builder.
    pub enum OutputBuilder {
        Blocking(Box<dyn BlockingOutputBuilder>),
        Async(Box<dyn AsyncOutputBuilder>),
    }

    /// Like [`OutputBuilder`] but with a [`Send`] bound on the builder.
    ///
    /// Use this type in the pipeline control loop.
    pub enum SendOutputBuilder {
        Blocking(Box<dyn BlockingOutputBuilder + Send>),
        Async(Box<dyn AsyncOutputBuilder + Send>),
    }

    impl From<SendOutputBuilder> for OutputBuilder {
        fn from(value: SendOutputBuilder) -> Self {
            match value {
                SendOutputBuilder::Blocking(b) => OutputBuilder::Blocking(b),
                SendOutputBuilder::Async(b) => OutputBuilder::Async(b),
            }
        }
    }

    pub struct BlockingOutputRegistration {
        pub name: OutputName,
        pub output: Box<dyn super::Output>,
    }

    pub struct AsyncOutputRegistration {
        pub name: OutputName,
        pub output: super::BoxedAsyncOutput,
    }

    /// Trait for builders of blocking outputs.
    ///
    ///  # Example
    /// ```
    /// use alumet::pipeline::elements::output::builder::{BlockingOutputBuilder, BlockingOutputRegistration, BlockingOutputBuildContext};
    /// use alumet::pipeline::{trigger, Output};
    ///
    /// fn build_my_output() -> anyhow::Result<Box<dyn Output>> {
    ///     todo!("build a new output")
    /// }
    ///
    /// let builder: &dyn BlockingOutputBuilder = &|ctx: &mut dyn BlockingOutputBuildContext| {
    ///     let output = build_my_output()?;
    ///     Ok(BlockingOutputRegistration {
    ///         name: ctx.output_name("my-output"),
    ///         output,
    ///     })
    /// };
    /// ```
    pub trait BlockingOutputBuilder:
        FnOnce(&mut dyn BlockingOutputBuildContext) -> anyhow::Result<BlockingOutputRegistration>
    {
    }
    impl<F> BlockingOutputBuilder for F where
        F: FnOnce(&mut dyn BlockingOutputBuildContext) -> anyhow::Result<BlockingOutputRegistration>
    {
    }

    pub trait AsyncOutputBuilder:
        FnOnce(&mut dyn AsyncOutputBuildContext, AsyncOutputStream) -> anyhow::Result<AsyncOutputRegistration>
    {
    }
    impl<F> AsyncOutputBuilder for F where
        F: FnOnce(&mut dyn AsyncOutputBuildContext, AsyncOutputStream) -> anyhow::Result<AsyncOutputRegistration>
    {
    }

    /// Context provided when building new outputs.
    pub(super) struct OutputBuildContext<'a> {
        pub(super) metrics: &'a MetricRegistry,
        pub(super) namegen: &'a mut PluginElementNamespace,
        pub(super) runtime: runtime::Handle,
    }

    pub trait BlockingOutputBuildContext {
        fn output_name(&mut self, name: &str) -> OutputName;

        fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)>;
    }

    pub trait AsyncOutputBuildContext {
        fn output_name(&mut self, name: &str) -> OutputName;

        fn async_runtime(&self) -> &tokio::runtime::Handle;
    }

    impl BlockingOutputBuildContext for OutputBuildContext<'_> {
        fn output_name(&mut self, name: &str) -> OutputName {
            OutputName(self.namegen.insert_deduplicate(name))
        }

        fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)> {
            self.metrics.by_name(name)
        }
    }

    impl AsyncOutputBuildContext for OutputBuildContext<'_> {
        fn output_name(&mut self, name: &str) -> OutputName {
            BlockingOutputBuildContext::output_name(self, name)
        }

        fn async_runtime(&self) -> &tokio::runtime::Handle {
            &self.runtime
        }
    }
}
