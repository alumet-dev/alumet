//! Implementation and control of output tasks.

use std::{fmt, ops::ControlFlow};

use tokio::{sync::broadcast, task::JoinSet};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{registry::SharedRegistryReader, Output, OutputContext, WriteError},
};

use super::versioned::Versioned;

pub struct OutputControl {
    tasks: JoinSet<anyhow::Result<()>>,
    configs: Vec<(OutputName, Versioned<TaskConfig>)>,
}

impl OutputControl {
    pub fn handle_message(&mut self, msg: ControlMessage) {
        for (name, output_config) in &mut self.configs {
            if name.matches(&msg.selector) {
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

#[derive(PartialEq, Eq)]
pub struct OutputName {
    plugin: String,
    output: String,
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

pub enum OutputSelector {
    Single(OutputName),
    Plugin(String),
    All,
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

async fn run(
    name: OutputName,
    mut output: Box<dyn Output>,
    mut rx: broadcast::Receiver<MeasurementBuffer>,
    mut versioned_config: Versioned<TaskConfig>,
    metrics_reader: SharedRegistryReader,
) -> anyhow::Result<()> {
    fn write_measurements(
        name: &OutputName,
        output: &mut dyn Output,
        ctx: OutputContext,
        m: Result<MeasurementBuffer, broadcast::error::RecvError>,
    ) -> anyhow::Result<ControlFlow<()>> {
        match m {
            Ok(measurements) => match output.write(&measurements, &ctx) {
                Ok(()) => Ok(ControlFlow::Continue(())),
                Err(WriteError::CanRetry(e)) => {
                    log::error!("Non-fatal error when writing to {name} (will retry): {e:#}");
                    Ok(ControlFlow::Continue(()))
                }
                Err(WriteError::Fatal(e)) => {
                    log::error!("Fatal error when writing to {name} (will stop running): {e:?}");
                    return Err(e.context(format!("fatal error when writing to {name}")));
                }
            },
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
            new_config = versioned_config.read_changed() => {
                let new_state = new_config.state;
                match new_state {
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
                let ctx = OutputContext{ metrics: &metrics_reader.read() };
                let res = write_measurements(&name, &mut *output, ctx, measurements)?;
                if res.is_break() {
                    break
                }
            }
        }
    }
    Ok(())
}
