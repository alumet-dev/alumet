//! Implementation and control of output tasks.

use std::fmt;

use tokio::{sync::broadcast, task::JoinSet};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{Output, WriteError},
};

use super::versioned::Versioned;

pub struct OutputControl {
    tasks: JoinSet<anyhow::Result<()>>,
    config: Versioned<TaskConfig>,
}

#[derive(PartialEq, Eq)]
pub struct OutputName {
    plugin: String,
    output: String,
}

pub enum OutputSelector {
    Single(OutputName),
    Plugin(String),
    All,
}

impl OutputName {
    pub fn matches(&self, selector: &OutputSelector) -> bool {
        match selector {
            OutputSelector::Single(full_name) => self == full_name,
            OutputSelector::Plugin(plugin_name) => &self.plugin == plugin_name,
            OutputSelector::All => true,
        }
    }
}

impl fmt::Display for OutputName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.plugin, self.output)
    }
}

struct TaskConfig {
    state: TaskState,
}

/// State of a (managed) output task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
pub enum TaskState {
    Run,
    Pause,
    Stop,
}

async fn run(
    name: OutputName,
    mut output: Box<dyn Output>,
    mut rx: broadcast::Receiver<MeasurementBuffer>,
    mut versioned_config: Versioned<TaskConfig>,
) -> anyhow::Result<()> {

    fn write_measurements(
        name: &OutputName,
        output: &mut dyn Output,
        m: Result<MeasurementBuffer, broadcast::error::RecvError>,
    ) -> anyhow::Result<()> {
        let ctx = todo!();
        match m {
            Ok(measurements) => match output.write(&measurements, ctx) {
                Ok(()) => Ok(()),
                Err(WriteError::CanRetry(e)) => {
                    log::error!("Non-fatal error when writing to {name} (will retry): {e:#}");
                    Ok(())
                }
                Err(WriteError::Fatal(e)) => {
                    log::error!("Fatal error when writing to {name} (will stop running): {e:?}");
                    return Err(e.context(format!("fatal error when writing to {name}")));
                }
            },
            Err(broadcast::error::RecvError::Lagged(n)) => {
                log::warn!("Output {name} is too slow, it lost the oldest {n} messages.");
                Ok(())
            }
            Err(broadcast::error::RecvError::Closed) => {
                log::warn!("The channel connected to output {name} was closed, it will now stop.");
                Ok(())
            }
        }
    }

    let mut receive = true;
    loop {
        tokio::select! {
            new_config = versioned_config.read_changed() => {
                let new_state = new_config.state;
                match new_state {
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
                write_measurements(&name, &mut *output, measurements)?;
            }
        }
    }
    Ok(())
}
