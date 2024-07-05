//! Implementation and control of output tasks.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use tokio::runtime;
use tokio::task::JoinError;
use tokio::{sync::broadcast, task::JoinSet};

use crate::measurement::MeasurementBuffer;
use crate::metrics::MetricRegistry;
use crate::pipeline::util::naming::{NameGenerator, OutputName, ScopedNameGenerator};
use crate::pipeline::{builder, PluginName};

use super::super::builder::elements::OutputBuilder;
use super::super::registry;
use super::super::util::versioned::Versioned;
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
    configs: Vec<(OutputName, Versioned<TaskConfig>)>,

    tx: broadcast::Sender<MeasurementBuffer>,

    /// Handle of the "normal" async runtime. Used for creating new outputs.
    rt_normal: runtime::Handle,

    metrics: registry::MetricReader,
}

struct BuildContext<'a> {
    metrics: &'a MetricRegistry,
    namegen: &'a mut ScopedNameGenerator,
}

impl OutputControl {
    pub fn new(
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: runtime::Handle,
        metrics: registry::MetricReader,
    ) -> Self {
        Self {
            tasks: TaskManager {
                spawned_tasks: JoinSet::new(),
                configs: Vec::new(),
                tx,
                rt_normal,
                metrics: metrics.clone(),
            },
            names: NameGenerator::new(),
            metrics,
        }
    }

    pub fn create_outputs(&mut self, outputs: Vec<(PluginName, Box<dyn OutputBuilder>)>) {
        let metrics = self.metrics.blocking_read();
        for (plugin, builder) in outputs {
            let mut ctx = BuildContext {
                metrics: &metrics,
                namegen: self.names.namegen_for_scope(&plugin),
            };
            self.tasks.create_output(&mut ctx, builder);
        }
    }

    pub fn create_output(&mut self, plugin: PluginName, builder: Box<dyn OutputBuilder>) {
        let metrics = self.metrics.blocking_read();
        let mut ctx = BuildContext {
            metrics: &metrics,
            namegen: self.names.namegen_for_scope(&plugin),
        };
        self.tasks.create_output(&mut ctx, builder);
    }

    pub fn handle_message(&mut self, msg: ControlMessage) {
        self.tasks.reconfigure(msg);
    }
    
    pub async fn join_next_task(&mut self) -> Option<Result<anyhow::Result<()>, JoinError>> {
        self.tasks.spawned_tasks.join_next().await
    }

    pub fn shutdown(mut self) {
        // Outputs naturally close when the input channel is closed,
        // but that only works when the output is running.
        // If the output is paused, it needs to be stopped with a command.
        let stop_msg = ControlMessage {
            selector: OutputSelector::All,
            new_state: OutputState::Stop,
        };
        self.handle_message(stop_msg);
    }
}

impl TaskManager {
    fn create_output(&mut self, ctx: &mut BuildContext, builder: Box<dyn OutputBuilder>) {
        let reg = builder(ctx);
        let rx = self.tx.subscribe();
        let config = Versioned::new(TaskConfig {
            state: OutputState::Run,
        });
        let metrics = self.metrics.clone();
        let guarded_output = Arc::new(Mutex::new(reg.output));
        let task = run_output(reg.name, guarded_output, rx, config, metrics);
        self.spawned_tasks.spawn_on(task, &self.rt_normal);
    }

    fn reconfigure(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.configs {
            if msg.selector.matches(name) {
                output_config.update(|config| config.state = msg.new_state);
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
}

pub enum OutputSelector {
    Single(OutputName),
    Plugin(String),
    All,
}

impl OutputSelector {
    pub fn matches(&self, name: &OutputName) -> bool {
        match self {
            OutputSelector::Single(full_name) => name == full_name,
            OutputSelector::Plugin(plugin_name) => &name.plugin == plugin_name,
            OutputSelector::All => true,
        }
    }
}

pub struct ControlMessage {
    pub selector: OutputSelector,
    pub new_state: OutputState,
}

struct TaskConfig {
    state: OutputState,
}

/// State of a (managed) output task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum OutputState {
    Run,
    Pause,
    Stop,
}

async fn run_output(
    name: OutputName,
    guarded_output: Arc<Mutex<Box<dyn Output>>>,
    mut rx: broadcast::Receiver<MeasurementBuffer>,
    mut versioned_config: Versioned<TaskConfig>,
    metrics_reader: registry::MetricReader,
) -> anyhow::Result<()> {
    async fn write_measurements(
        name: &OutputName,
        output: Arc<Mutex<Box<dyn Output>>>,
        metrics_r: registry::MetricReader,
        m: Result<MeasurementBuffer, broadcast::error::RecvError>,
    ) -> anyhow::Result<ControlFlow<()>> {
        match m {
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
                        return Err(e.context(format!("fatal error when writing to {name}")));
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                log::warn!("Output {name} is too slow, it lost the oldest {n} messages.");
                Ok(ControlFlow::Continue(()))
            }
            Err(broadcast::error::RecvError::Closed) => {
                log::warn!("The channel connected to output {name} was closed, it will now stop.");
                Ok(ControlFlow::Break(()))
            }
        }
    }

    let mut receive = true;
    loop {
        tokio::select! {
            _ = versioned_config.changed() => {
                // We cannot use read_changed() here because versioned::Ref cannot be help across await points
                // (it uses a regular MutexGuard).
                let new_config = versioned_config.read();
                match new_config.state {
                    OutputState::Run => {
                        receive = true;
                    }
                    OutputState::Pause => {
                        receive = false;
                    }
                    OutputState::Stop => {
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
