//! Implementation and control of output tasks.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use tokio::runtime;
use tokio::{sync::broadcast, task::JoinSet};

use crate::measurement::MeasurementBuffer;
use crate::metrics::MetricRegistry;
use crate::pipeline::builder::context::OutputBuildContext;
use crate::pipeline::util::naming::{NameGenerator, OutputName};
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

pub struct OutputControl {
    tasks: JoinSet<anyhow::Result<()>>,
    configs: Vec<(OutputName, Versioned<TaskConfig>)>,

    tx: broadcast::Sender<MeasurementBuffer>,

    /// Handle of the "normal" async runtime. Used for creating new outputs.
    rt_normal: runtime::Handle,

    /// Generates deduplicated names for new outputs.
    namegen_by_plugin: HashMap<PluginName, NameGenerator>,

    /// Read-only access to the metrics.
    metrics: registry::MetricReader,
}

impl OutputControl {
    pub fn new(
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: runtime::Handle,
        metrics: registry::MetricReader,
    ) -> Self {
        Self {
            tasks: JoinSet::new(),
            configs: Vec::with_capacity(4),
            tx,
            rt_normal,
            namegen_by_plugin: HashMap::new(),
            metrics,
        }
    }

    pub fn create_output(&mut self, plugin: PluginName, builder: Box<dyn OutputBuilder>) {
        let metrics = self.metrics.blocking_read(); // TODO how to pass this to create_output?
        let mut ctx = BuildContext {
            metrics: &metrics,
            namegen: self
                .namegen_by_plugin
                .entry(plugin.clone())
                .or_insert_with(|| NameGenerator::new(plugin)),
        };
        let reg = builder(&mut ctx);
        let rx = self.tx.subscribe();
        let config = Versioned::new(TaskConfig {
            state: OutputState::Run,
        });
        let metrics = self.metrics.clone();
        let guarded_output = Arc::new(Mutex::new(reg.output));
        let task = run_output(reg.name, guarded_output, rx, config, metrics);
        self.tasks.spawn_on(task, &self.rt_normal);
    }

    pub fn handle_message(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.configs {
            if msg.selector.matches(name) {
                output_config.update(|config| config.state = msg.new_state);
            }
        }
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

struct BuildContext<'a> {
    metrics: &'a MetricRegistry,
    namegen: &'a mut NameGenerator,
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
