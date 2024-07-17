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

#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use std::{any::Any, time::Duration};

    use alumet::pipeline::{
        control::ControlMessage,
        elements::{output, source, transform},
        matching::{NamePattern, NamePatterns, OutputSelector, SourceSelector, TransformSelector},
        trigger::TriggerSpec,
    };

    use super::{parse, Command};

    #[test]
    fn control_source() -> anyhow::Result<()> {
        assert_control_eq(
            parse("control my-plugin/sources/my-source pause")?,
            vec![ControlMessage::Source(source::ControlMessage::Configure(
                source::ConfigureMessage {
                    selector: SourceSelector::from(NamePatterns {
                        plugin: NamePattern::Exact(String::from("my-plugin")),
                        name: NamePattern::Exact(String::from("my-source")),
                    }),
                    command: source::ConfigureCommand::Pause,
                },
            ))],
        );
        assert_control_eq(
            parse("control */sources/* resume")?,
            vec![ControlMessage::Source(source::ControlMessage::Configure(
                source::ConfigureMessage {
                    selector: SourceSelector::all(),
                    command: source::ConfigureCommand::Resume,
                },
            ))],
        );
        assert_control_eq(
            parse("control */src/* stop")?,
            vec![ControlMessage::Source(source::ControlMessage::Configure(
                source::ConfigureMessage {
                    selector: SourceSelector::all(),
                    command: source::ConfigureCommand::Stop,
                },
            ))],
        );
        assert_control_eq(
            parse("control sources trigger-now")?,
            vec![ControlMessage::Source(source::ControlMessage::TriggerManually(
                source::TriggerMessage {
                    selector: SourceSelector::all(),
                },
            ))],
        );
        assert_control_eq(
            parse("control */out/* stop")?,
            vec![ControlMessage::Output(output::ControlMessage {
                selector: OutputSelector::all(),
                new_state: output::TaskState::Stop,
            })],
        );
        assert_control_eq(
            parse("control */tra/* enable")?,
            vec![ControlMessage::Transform(transform::ControlMessage {
                selector: TransformSelector::all(),
                new_state: transform::TaskState::Enabled,
            })],
        );
        assert_control_eq(
            parse("control * pause")?,
            vec![
                ControlMessage::Source(source::ControlMessage::Configure(source::ConfigureMessage {
                    selector: SourceSelector::all(),
                    command: source::ConfigureCommand::Pause,
                })),
                ControlMessage::Transform(transform::ControlMessage {
                    selector: TransformSelector::all(),
                    new_state: transform::TaskState::Disabled,
                }),
                ControlMessage::Output(output::ControlMessage {
                    selector: OutputSelector::all(),
                    new_state: output::TaskState::Pause,
                }),
            ],
        );
        assert_control_eq(
            parse("control sources set-period 10ms")?,
            vec![ControlMessage::Source(source::ControlMessage::Configure(
                source::ConfigureMessage {
                    selector: SourceSelector::all(),
                    command: source::ConfigureCommand::SetTrigger(TriggerSpec::at_interval(Duration::from_millis(10))),
                },
            ))],
        );
        Ok(())
    }

    fn assert_control_eq(cmd: Command, msg: Vec<ControlMessage>) {
        match &cmd {
            Command::Control(messages) => {
                for (a, b) in messages.iter().zip(&msg) {
                    if !control_message_eq(&a, &b) {
                        panic!("wrong command {cmd:?}, expected Control({msg:?})")
                    }
                }
            }
            _ => panic!("wrong command {cmd:?}, expected Control({msg:?})"),
        }
    }

    fn control_message_eq(a: &ControlMessage, b: &ControlMessage) -> bool {
        fn source_msg_eq(a: &source::ControlMessage, b: &source::ControlMessage) -> bool {
            use alumet::pipeline::builder::elements::SendSourceBuilder;
            use source::ConfigureCommand;

            match (a, b) {
                (source::ControlMessage::Configure(c1), source::ControlMessage::Configure(c2)) => {
                    c1.selector == c2.selector
                        && match (&c1.command, &c2.command) {
                            (ConfigureCommand::Pause, ConfigureCommand::Pause) => true,
                            (ConfigureCommand::Resume, ConfigureCommand::Resume) => true,
                            (ConfigureCommand::Stop, ConfigureCommand::Stop) => true,
                            (ConfigureCommand::SetTrigger(t1), ConfigureCommand::SetTrigger(t2)) => t1 == t2,
                            _ => false,
                        }
                }
                (source::ControlMessage::Create(c1), source::ControlMessage::Create(c2)) => {
                    c1.plugin == c2.plugin
                        && match (&c1.builder, &c2.builder) {
                            (SendSourceBuilder::Managed(b1), SendSourceBuilder::Managed(b2)) => {
                                b1.type_id() == b2.type_id()
                            }
                            (SendSourceBuilder::Autonomous(b1), SendSourceBuilder::Autonomous(b2)) => {
                                b1.type_id() == b2.type_id()
                            }
                            _ => false,
                        }
                }
                (source::ControlMessage::TriggerManually(t1), source::ControlMessage::TriggerManually(t2)) => {
                    t1.selector == t2.selector
                }
                _ => false,
            }
        }

        fn transform_msg_eq(a: &transform::ControlMessage, b: &transform::ControlMessage) -> bool {
            a.selector == b.selector && a.new_state == b.new_state
        }

        fn output_msg_eq(a: &output::ControlMessage, b: &output::ControlMessage) -> bool {
            a.selector == b.selector && a.new_state == b.new_state
        }

        match (a, b) {
            (ControlMessage::Source(s1), ControlMessage::Source(s2)) => source_msg_eq(s1, s2),
            (ControlMessage::Transform(t1), ControlMessage::Transform(t2)) => transform_msg_eq(t1, t2),
            (ControlMessage::Output(o1), ControlMessage::Output(o2)) => output_msg_eq(o1, o2),
            _ => false,
        }
    }
}
