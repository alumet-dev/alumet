//! Command parsing.

use std::str::FromStr;

use alumet::pipeline::control::{AnonymousControlHandle, ControlError};
use alumet::pipeline::matching::{ElementSelector, OutputSelector, SourceSelector, TransformSelector};
use alumet::pipeline::{
    control::ControlMessage,
    elements::{output, source, transform},
    trigger,
};
use anyhow::{anyhow, Context};
use humantime::parse_duration;

pub enum Command {
    Control(Vec<ControlMessage>),
    Shutdown,
}

impl Command {
    pub async fn run(self, handle: &AnonymousControlHandle) -> Result<(), ControlError> {
        match self {
            Command::Control(messages) => {
                for msg in messages {
                    handle.send(msg).await?;
                }
                Ok(())
            }
            Command::Shutdown => {
                handle.shutdown();
                Ok(())
            }
        }
    }
}

/// Parses a command from a string.
///
/// ## Available commands
///
/// - `shutdown` or `stop`: shutdowns the measurement pipeline
/// - `control <SELECTOR> [ARGS...]`: reconfigures a part of the pipeline (see below)
///
/// ### Control arguments
///
/// The available options for `control` depend on the kind of element that the selector targets.
///
/// Options available on any element (sources, transforms and outputs):
///     - `pause` or `disable`: pauses a source, transform or output
///     - `resume` or `enable`: resumes a source, transform or output
///
/// Options available on sources and outputs (not transforms):
///     - `stop`: stops and destroys the source or output
///
/// Options available on sources only:
///     - `set-period <Duration>`: changes the time period between two measurements (only works if the source is a "managed" source)
///     - `trigger-now`: requests Alumet to poll the source (only works if the source enables manual trigger)
///
pub fn parse(command: &str) -> anyhow::Result<Command> {
    fn parse_control_args(selector: ElementSelector, args: &[&str]) -> anyhow::Result<Vec<ControlMessage>> {
        fn msg_config_source(selector: SourceSelector, command: source::ConfigureCommand) -> ControlMessage {
            ControlMessage::Source(source::ControlMessage::Configure(source::ConfigureMessage {
                selector,
                command,
            }))
        }

        fn msg_config_transform(selector: TransformSelector, new_state: transform::TaskState) -> ControlMessage {
            ControlMessage::Transform(transform::ControlMessage { selector, new_state })
        }

        fn msg_config_output(selector: OutputSelector, new_state: output::TaskState) -> ControlMessage {
            ControlMessage::Output(output::ControlMessage { selector, new_state })
        }

        match args {
            [] => Err(anyhow!("missing arguments after the selector")),
            ["pause"] | ["disable"] => match selector {
                ElementSelector::Source(sel) => Ok(vec![msg_config_source(sel, source::ConfigureCommand::Pause)]),
                ElementSelector::Transform(sel) => Ok(vec![msg_config_transform(sel, transform::TaskState::Disabled)]),
                ElementSelector::Output(sel) => Ok(vec![msg_config_output(sel, output::TaskState::Pause)]),
                ElementSelector::Any(sel) => {
                    let for_sources = msg_config_source(sel.clone().into(), source::ConfigureCommand::Pause);
                    let for_transforms = msg_config_transform(sel.clone().into(), transform::TaskState::Disabled);
                    let for_outputs = msg_config_output(sel.into(), output::TaskState::Pause);
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["resume"] | ["enable"] => match selector {
                ElementSelector::Source(sel) => Ok(vec![msg_config_source(sel, source::ConfigureCommand::Resume)]),
                ElementSelector::Transform(sel) => Ok(vec![msg_config_transform(sel, transform::TaskState::Enabled)]),
                ElementSelector::Output(sel) => Ok(vec![msg_config_output(sel, output::TaskState::Run)]),
                ElementSelector::Any(sel) => {
                    let for_sources = msg_config_source(sel.clone().into(), source::ConfigureCommand::Resume);
                    let for_transforms = msg_config_transform(sel.clone().into(), transform::TaskState::Enabled);
                    let for_outputs = msg_config_output(sel.into(), output::TaskState::Run);
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["stop"] => match selector {
                ElementSelector::Source(sel) => Ok(vec![msg_config_source(sel, source::ConfigureCommand::Stop)]),
                ElementSelector::Output(sel) => Ok(vec![msg_config_output(sel, output::TaskState::Stop)]),
                _ => Err(anyhow!(
                    "invalid control 'stop': it can only be applied to sources and outputs"
                )),
            },
            ["set-period", period] | ["set-poll-interval", period] => match selector {
                ElementSelector::Source(sel) => {
                    let poll_interval = parse_duration(period)?;
                    let spec = trigger::TriggerSpec::at_interval(poll_interval);
                    Ok(vec![msg_config_source(sel, source::ConfigureCommand::SetTrigger(spec))])
                }
                _ => Err(anyhow!(
                    "invalid control 'set-period': it can only be applied to sources"
                )),
            },
            ["trigger-now"] => match selector {
                ElementSelector::Source(sel) => {
                    let msg = source::ControlMessage::TriggerManually(source::TriggerMessage { selector: sel });
                    Ok(vec![ControlMessage::Source(msg)])
                }
                _ => Err(anyhow!(
                    "invalid control 'trigger-now': it can only be applied to sources"
                )),
            },
            _ => Err(anyhow!("invalid command")),
        }
    }

    let parts: Vec<&str> = command.split_ascii_whitespace().collect();
    match parts[0] {
        "shutdown" | "stop" => Ok(Command::Shutdown),
        "control" => {
            let selector = ElementSelector::from_str(
                parts
                    .get(1)
                    .context("invalid command 'control': missing argument 'selector'")?,
            )?;
            let messages =
                parse_control_args(selector, &parts[2..]).with_context(|| format!("invalid command '{command}'"))?;
            Ok(Command::Control(messages))
        }
        _ => Err(anyhow!(
            "unknown command '{command}'; available commands are 'shutdown' or 'control'"
        )),
    }
}
