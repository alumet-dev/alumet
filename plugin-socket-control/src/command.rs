//! Command parsing.

use std::str::FromStr;

use alumet::pipeline::control::message::matching::{OutputMatcher, SourceMatcher, TransformMatcher};
use alumet::pipeline::control::{error::ControlError, AnonymousControlHandle, ControlMessage};
use alumet::pipeline::matching::{
    ElementNamePattern, OutputNamePattern, SourceNamePattern, StringPattern, TransformNamePattern,
};
use alumet::pipeline::naming::parsing::parse_kind;
use alumet::pipeline::naming::ElementKind;
use alumet::pipeline::{
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
/// - `control <PATTERN> [ARGS...]`: reconfigures a part of the pipeline (see below)
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
    fn parse_control_args(pat: ElementNamePattern, args: &[&str]) -> anyhow::Result<Vec<ControlMessage>> {
        fn msg_config_source(pat: SourceNamePattern, command: source::control::ConfigureCommand) -> ControlMessage {
            ControlMessage::Source(source::control::ControlMessage::Configure(
                source::control::ConfigureMessage {
                    matcher: SourceMatcher::Name(pat),
                    command,
                },
            ))
        }

        fn msg_config_transform(pat: TransformNamePattern, new_state: transform::control::TaskState) -> ControlMessage {
            ControlMessage::Transform(transform::control::ControlMessage {
                matcher: TransformMatcher::Name(pat),
                new_state,
            })
        }

        fn msg_config_output(pat: OutputNamePattern, new_state: output::control::TaskState) -> ControlMessage {
            ControlMessage::Output(output::control::ControlMessage {
                matcher: OutputMatcher::Name(pat),
                new_state,
            })
        }

        match args {
            [] => Err(anyhow!("missing arguments after the selector")),
            ["pause"] | ["disable"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![msg_config_source(
                    pat.try_into().unwrap(),
                    source::control::ConfigureCommand::Pause,
                )]),
                Some(ElementKind::Transform) => Ok(vec![msg_config_transform(
                    pat.try_into().unwrap(),
                    transform::control::TaskState::Disabled,
                )]),
                Some(ElementKind::Output) => Ok(vec![msg_config_output(
                    pat.try_into().unwrap(),
                    output::control::TaskState::Pause,
                )]),
                None => {
                    let for_sources = msg_config_source(
                        pat.clone().try_into().unwrap(),
                        source::control::ConfigureCommand::Pause,
                    );
                    let for_transforms =
                        msg_config_transform(pat.clone().try_into().unwrap(), transform::control::TaskState::Disabled);
                    let for_outputs = msg_config_output(pat.try_into().unwrap(), output::control::TaskState::Pause);
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["resume"] | ["enable"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![msg_config_source(
                    pat.try_into().unwrap(),
                    source::control::ConfigureCommand::Resume,
                )]),
                Some(ElementKind::Transform) => Ok(vec![msg_config_transform(
                    pat.try_into().unwrap(),
                    transform::control::TaskState::Enabled,
                )]),
                Some(ElementKind::Output) => Ok(vec![msg_config_output(
                    pat.try_into().unwrap(),
                    output::control::TaskState::Run,
                )]),
                None => {
                    let for_sources = msg_config_source(
                        pat.clone().try_into().unwrap(),
                        source::control::ConfigureCommand::Resume,
                    );
                    let for_transforms =
                        msg_config_transform(pat.clone().try_into().unwrap(), transform::control::TaskState::Enabled);
                    let for_outputs = msg_config_output(pat.try_into().unwrap(), output::control::TaskState::Run);
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["stop"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![msg_config_source(
                    pat.try_into().unwrap(),
                    source::control::ConfigureCommand::Stop,
                )]),
                Some(ElementKind::Output) => Ok(vec![msg_config_output(
                    pat.try_into().unwrap(),
                    output::control::TaskState::StopNow,
                )]),
                _ => Err(anyhow!(
                    "invalid control 'stop': it can only be applied to sources and outputs"
                )),
            },
            ["set-period", period] | ["set-poll-interval", period] => match &pat.kind {
                Some(ElementKind::Source) => {
                    let poll_interval = parse_duration(period)?;
                    let spec = trigger::TriggerSpec::at_interval(poll_interval);
                    Ok(vec![msg_config_source(
                        pat.try_into().unwrap(),
                        source::control::ConfigureCommand::SetTrigger(spec),
                    )])
                }
                _ => Err(anyhow!(
                    "invalid control 'set-period': it can only be applied to sources"
                )),
            },
            ["trigger-now"] => match &pat.kind {
                Some(ElementKind::Source) => {
                    let msg = source::control::ControlMessage::TriggerManually(source::control::TriggerMessage {
                        matcher: SourceMatcher::Name(pat.try_into().unwrap()),
                    });
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
            let pat = parts
                .get(1)
                .context("invalid command 'control': missing argument 'selector'")?;
            let pattern = parse_pattern(pat)?;
            let messages =
                parse_control_args(pattern, &parts[2..]).with_context(|| format!("invalid command '{command}'"))?;
            Ok(Command::Control(messages))
        }
        _ => Err(anyhow!(
            "unknown command '{command}'; available commands are 'shutdown' or 'control'"
        )),
    }
}

pub fn parse_pattern(pat: &str) -> anyhow::Result<ElementNamePattern> {
    let parts: Vec<_> = pat.splitn(3, '/').collect();
    match parts[..] {
        [kind, plugin_pat, element_pat] => {
            let kind = parse_kind(kind).with_context(|| format!("bad kind: '{kind}'"))?;
            let plugin = StringPattern::from_str(plugin_pat).with_context(|| format!("bad pattern: '{plugin_pat}'"))?;
            let element =
                StringPattern::from_str(element_pat).with_context(|| format!("bad pattern: '{element_pat}'"))?;
            Ok(ElementNamePattern { kind, plugin, element })
        }
        [kind] => {
            let kind = parse_kind(kind).with_context(|| format!("bad kind: '{kind}'"))?;
            Ok(ElementNamePattern {
                kind,
                plugin: StringPattern::Any,
                element: StringPattern::Any,
            })
        }
        _ => Err(anyhow!("bad pattern, expected kind/plugin/element but got '{pat}'")),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{parse, Command};
    use alumet::pipeline::control::message::matching::{OutputMatcher, SourceMatcher, TransformMatcher};
    use alumet::pipeline::elements::source::control::TriggerMessage;
    use alumet::pipeline::matching::{OutputNamePattern, TransformNamePattern};
    use alumet::pipeline::{
        control::ControlMessage,
        elements::{output, source, transform},
        matching::SourceNamePattern,
        trigger::TriggerSpec,
    };
    use output::control::ControlMessage as OutputControlMessage;
    use regex::Regex;
    use source::control::{ConfigureCommand, ConfigureMessage, ControlMessage as SourceControlMessage};
    use transform::control::ControlMessage as TransformControlMessage;

    #[test]
    fn control_source_exact() {
        assert_control_eq(
            parse("control sources/my-plugin/my-source pause").unwrap(),
            vec![ControlMessage::Source(SourceControlMessage::Configure(
                ConfigureMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::exact("my-plugin", "my-source")),
                    command: ConfigureCommand::Pause,
                },
            ))],
        );
    }

    #[test]
    fn control_source_any() {
        assert_control_eq(
            parse("control sources/*/* resume").unwrap(),
            vec![ControlMessage::Source(SourceControlMessage::Configure(
                ConfigureMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
                    command: ConfigureCommand::Resume,
                },
            ))],
        );
    }

    #[test]
    fn control_source_any_shortened() {
        assert_control_eq(
            parse("control src/*/* stop").unwrap(),
            vec![ControlMessage::Source(SourceControlMessage::Configure(
                ConfigureMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
                    command: ConfigureCommand::Stop,
                },
            ))],
        );
    }

    #[test]
    fn control_source_trigger() {
        assert_control_eq(
            parse("control sources trigger-now").unwrap(),
            vec![ControlMessage::Source(SourceControlMessage::TriggerManually(
                TriggerMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
                },
            ))],
        );
    }
    #[test]
    fn control_output_stop() {
        assert_control_eq(
            parse("control out/*/* stop").unwrap(),
            vec![ControlMessage::Output(OutputControlMessage {
                matcher: OutputMatcher::Name(OutputNamePattern::wildcard()),
                new_state: output::control::TaskState::StopNow,
            })],
        );
    }
    #[test]
    fn control_transform_enable() {
        assert_control_eq(
            parse("control tra/*/* enable").unwrap(),
            vec![ControlMessage::Transform(TransformControlMessage {
                matcher: TransformMatcher::Name(TransformNamePattern::wildcard()),
                new_state: transform::control::TaskState::Enabled,
            })],
        );
    }

    #[test]
    fn control_all_pause() {
        assert_control_eq(
            parse("control * pause").unwrap(),
            vec![
                ControlMessage::Source(SourceControlMessage::Configure(ConfigureMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
                    command: ConfigureCommand::Pause,
                })),
                ControlMessage::Transform(TransformControlMessage {
                    matcher: TransformMatcher::Name(TransformNamePattern::wildcard()),
                    new_state: transform::control::TaskState::Disabled,
                }),
                ControlMessage::Output(OutputControlMessage {
                    matcher: OutputMatcher::Name(OutputNamePattern::wildcard()),
                    new_state: output::control::TaskState::Pause,
                }),
            ],
        );
    }

    #[test]
    fn control_source_set_poll_interval() {
        assert_control_eq(
            parse("control sources set-period 10ms").unwrap(),
            vec![ControlMessage::Source(SourceControlMessage::Configure(
                ConfigureMessage {
                    matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
                    command: ConfigureCommand::SetTrigger(TriggerSpec::at_interval(Duration::from_millis(10))),
                },
            ))],
        );
    }

    fn assert_control_eq(cmd: Command, msg: Vec<ControlMessage>) {
        let regex_instant = Regex::new(r#"Instant \{ .+ \}"#).expect("regex should be valid");

        match &cmd {
            Command::Control(messages) => {
                for (a, b) in messages.iter().zip(&msg) {
                    // It's too cumbersome to manually implement partial equality between control messages,
                    // use Debug and ignore the parts that we don't want.
                    let a = format!("{a:?}");
                    let b = format!("{b:?}");
                    let a = regex_instant.replace(&a, "Instant { opaque }");
                    let b = regex_instant.replace(&b, "Instant { opaque }");
                    pretty_assertions::assert_str_eq!(a, b);
                }
            }
            _ => panic!("wrong command {cmd:?},\nexpected Control({msg:?})"),
        }
    }
}
