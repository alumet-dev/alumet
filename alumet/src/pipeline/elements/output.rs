//! Implementation and control of output tasks.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use tokio::runtime;
use tokio::sync::Notify;
use tokio::task::{JoinError, JoinSet};

use crate::measurement::MeasurementBuffer;
use crate::metrics::MetricRegistry;
use crate::pipeline::util::channel;
use crate::pipeline::util::matching::OutputSelector;
use crate::pipeline::util::naming::{NameGenerator, OutputName, ScopedNameGenerator};
use crate::pipeline::{builder, PluginName};

use super::super::builder::elements::OutputBuilder;
use super::super::registry;
use super::error::WriteError;

/// Exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

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
    configs: Vec<(OutputName, Arc<SharedOutputConfig>)>,

    rx_provider: channel::ReceiverProvider,

    /// Handle of the "normal" async runtime. Used for creating new outputs.
    rt_normal: runtime::Handle,

    metrics: registry::MetricReader,
}

struct BuildContext<'a> {
    metrics: &'a MetricRegistry,
    namegen: &'a mut ScopedNameGenerator,
    runtime: runtime::Handle,
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
                configs: Vec::new(),
                rx_provider,
                rt_normal,
                metrics: metrics.clone(),
            },
            names: NameGenerator::new(),
            metrics,
        }
    }

    pub fn blocking_create_outputs(&mut self, outputs: Vec<(PluginName, Box<dyn OutputBuilder>)>) -> anyhow::Result<()> {
        let metrics = self.metrics.blocking_read();
        for (plugin, builder) in outputs {
            let mut ctx = BuildContext {
                metrics: &metrics,
                namegen: self.names.namegen_for_scope(&plugin),
                runtime: self.tasks.rt_normal.clone(),
            };
            self.tasks.create_output(&mut ctx, builder)?;
        }
        Ok(())
    }

    #[allow(unused)]
    pub async fn create_output(&mut self, plugin: PluginName, builder: Box<dyn OutputBuilder + Send>) {
        let metrics = self.metrics.read().await;
        let mut ctx = BuildContext {
            metrics: &metrics,
            namegen: self.names.namegen_for_scope(&plugin),
            runtime: self.tasks.rt_normal.clone(),
        };
        self.tasks.create_output(&mut ctx, builder);
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
            new_state: TaskState::Stop,
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
    fn create_output(&mut self, ctx: &mut BuildContext, builder: Box<dyn OutputBuilder>) -> anyhow::Result<()> {
        // Build the output.
        let reg = builder(ctx).context("output creation failed")?;

        // Create the necessary context.
        let rx = self.rx_provider.get(); // to receive measurements
        let metrics = self.metrics.clone(); // to read metric definitions

        // Create the task config.
        let config = Arc::new(SharedOutputConfig::new());
        self.configs.push((reg.name.clone(), config.clone()));

        // Put the output in a Mutex to overcome the lack of tokio::spawn_scoped.
        let guarded_output = Arc::new(Mutex::new(reg.output));

        // Spawn the task on the runtime.
        match rx {
            // Specialize on the kind of receiver at compile-time (for performance).
            channel::ReceiverEnum::Broadcast(rx) => {
                let task = run_output(reg.name, guarded_output, rx, metrics, config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
            channel::ReceiverEnum::Single(rx) => {
                let task = run_output(reg.name, guarded_output, rx, metrics, config);
                self.spawned_tasks.spawn_on(task, &self.rt_normal);
            }
        }

        Ok(())
    }

    fn reconfigure(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.configs {
            if msg.selector.matches(name) {
                output_config.set_state(msg.new_state);
            }
        }
    }
}

impl builder::context::OutputBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)> {
        self.metrics.by_name(name)
    }

    fn output_name(&mut self, name: &str) -> OutputName {
        self.namegen.output_name(name)
    }

    fn async_runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }
}

#[derive(Debug)]
pub struct ControlMessage {
    pub selector: OutputSelector,
    pub new_state: TaskState,
}

/// State of a (managed) output task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
#[repr(u8)]
pub enum TaskState {
    Run,
    Pause,
    Stop,
}

impl From<u8> for TaskState {
    fn from(value: u8) -> Self {
        const RUN: u8 = TaskState::Run as u8;
        const PAUSE: u8 = TaskState::Pause as u8;

        match value {
            RUN => TaskState::Run,
            PAUSE => TaskState::Pause,
            _ => TaskState::Stop,
        }
    }
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

async fn run_output<Rx: channel::MeasurementReceiver>(
    name: OutputName,
    guarded_output: Arc<Mutex<Box<dyn Output>>>,
    mut rx: Rx,
    metrics_reader: registry::MetricReader,
    config: Arc<SharedOutputConfig>,
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
                    TaskState::Stop => {
                        break; // stop the output
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
    Ok(())
}
