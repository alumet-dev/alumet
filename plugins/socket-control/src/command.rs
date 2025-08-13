//! Command parsing.

use std::str::FromStr;
use std::time::Duration;

use alumet::pipeline::control::AnonymousControlHandle;
use alumet::pipeline::control::handle::DispatchError;
use alumet::pipeline::control::request::{self, any::AnyAnonymousControlRequest};
use alumet::pipeline::elements::source::trigger::TriggerSpec;
use alumet::pipeline::matching::{
    ElementNamePattern, OutputNamePattern, SourceNamePattern, StringPattern, TransformNamePattern,
};
use alumet::pipeline::naming::ElementKind;
use alumet::pipeline::naming::parsing::parse_kind;

use anyhow::{Context, anyhow};
use humantime::parse_duration;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub enum Command {
    Control(Vec<AnyAnonymousControlRequest>),
    Shutdown,
}

impl Command {
    pub async fn run(self, handle: &AnonymousControlHandle) -> Result<(), DispatchError> {
        match self {
            Command::Control(messages) => {
                for msg in messages {
                    handle.dispatch(msg, COMMAND_TIMEOUT).await?;
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
    fn parse_control_args(pat: ElementNamePattern, args: &[&str]) -> anyhow::Result<Vec<AnyAnonymousControlRequest>> {
        match args {
            [] => Err(anyhow!("missing arguments after the selector")),
            ["pause"] | ["disable"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![
                    request::source(SourceNamePattern::try_from(pat).unwrap())
                        .disable()
                        .into(),
                ]),
                Some(ElementKind::Transform) => Ok(vec![
                    request::transform(TransformNamePattern::try_from(pat).unwrap())
                        .disable()
                        .into(),
                ]),
                Some(ElementKind::Output) => Ok(vec![
                    request::output(OutputNamePattern::try_from(pat).unwrap())
                        .disable()
                        .into(),
                ]),
                None => {
                    let for_sources = request::source(SourceNamePattern::try_from(pat.clone()).unwrap())
                        .disable()
                        .into();
                    let for_transforms = request::transform(TransformNamePattern::try_from(pat.clone()).unwrap())
                        .disable()
                        .into();
                    let for_outputs = request::output(OutputNamePattern::try_from(pat).unwrap())
                        .disable()
                        .into();
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["resume"] | ["enable"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![
                    request::source(SourceNamePattern::try_from(pat).unwrap())
                        .enable()
                        .into(),
                ]),
                Some(ElementKind::Transform) => Ok(vec![
                    request::transform(TransformNamePattern::try_from(pat).unwrap())
                        .enable()
                        .into(),
                ]),
                Some(ElementKind::Output) => Ok(vec![
                    request::output(OutputNamePattern::try_from(pat).unwrap())
                        .enable()
                        .into(),
                ]),
                None => {
                    let for_sources = request::source(SourceNamePattern::try_from(pat.clone()).unwrap())
                        .enable()
                        .into();
                    let for_transforms = request::transform(TransformNamePattern::try_from(pat.clone()).unwrap())
                        .enable()
                        .into();
                    let for_outputs = request::output(OutputNamePattern::try_from(pat).unwrap())
                        .enable()
                        .into();
                    Ok(vec![for_sources, for_transforms, for_outputs])
                }
            },
            ["stop"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![
                    request::source(SourceNamePattern::try_from(pat).unwrap()).stop().into(),
                ]),
                Some(ElementKind::Output) => Ok(vec![
                    request::output(OutputNamePattern::try_from(pat).unwrap())
                        .stop(request::RemainingDataStrategy::Write)
                        .into(),
                ]),
                _ => Err(anyhow!(
                    "invalid control 'stop': it can only be applied to sources and outputs"
                )),
            },
            ["set-period", period] | ["set-poll-interval", period] => match &pat.kind {
                Some(ElementKind::Source) => {
                    let poll_interval = parse_duration(period)?;
                    let spec = TriggerSpec::at_interval(poll_interval);
                    Ok(vec![
                        request::source(SourceNamePattern::try_from(pat).unwrap())
                            .set_trigger(spec)
                            .into(),
                    ])
                }
                _ => Err(anyhow!(
                    "invalid control 'set-period': it can only be applied to sources"
                )),
            },
            ["trigger-now"] => match &pat.kind {
                Some(ElementKind::Source) => Ok(vec![
                    request::source(SourceNamePattern::try_from(pat).unwrap())
                        .trigger_now()
                        .into(),
                ]),
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
    use regex::Regex;
    use std::time::Duration;

    use super::{Command, parse};
    use alumet::pipeline::control::matching::{OutputMatcher, SourceMatcher, TransformMatcher};
    use alumet::pipeline::control::request::{self, any::AnyAnonymousControlRequest};
    use alumet::pipeline::elements::source::trigger::TriggerSpec;
    use alumet::pipeline::matching::{OutputNamePattern, SourceNamePattern, TransformNamePattern};

    #[test]
    fn control_source_exact() {
        assert_control_eq(
            parse("control sources/my-plugin/my-source pause").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::exact("my-plugin", "my-source")))
                    .disable()
                    .into(),
            ],
        );
    }

    #[test]
    fn control_source_any() {
        assert_control_eq(
            parse("control sources/*/* resume").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::wildcard()))
                    .enable()
                    .into(),
            ],
        );
    }

    #[test]
    fn control_source_any_shortened() {
        assert_control_eq(
            parse("control src/*/* stop").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::wildcard()))
                    .stop()
                    .into(),
            ],
        );
    }

    #[test]
    fn control_source_trigger_now() {
        assert_control_eq(
            parse("control sources trigger-now").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::wildcard()))
                    .trigger_now()
                    .into(),
            ],
        );
    }
    #[test]
    fn control_output_stop() {
        assert_control_eq(
            parse("control out/*/* stop").unwrap(),
            vec![
                request::output(OutputMatcher::Name(OutputNamePattern::wildcard()))
                    .stop(request::RemainingDataStrategy::Write)
                    .into(),
            ],
        );
    }
    #[test]
    fn control_transform_enable() {
        assert_control_eq(
            parse("control tra/*/* enable").unwrap(),
            vec![
                request::transform(TransformMatcher::Name(TransformNamePattern::wildcard()))
                    .enable()
                    .into(),
            ],
        );
    }

    #[test]
    fn control_all_pause() {
        assert_control_eq(
            parse("control * pause").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::wildcard()))
                    .disable()
                    .into(),
                request::transform(TransformMatcher::Name(TransformNamePattern::wildcard()))
                    .disable()
                    .into(),
                request::output(OutputMatcher::Name(OutputNamePattern::wildcard()))
                    .disable()
                    .into(),
            ],
        );
    }

    #[test]
    fn control_source_set_poll_interval() {
        assert_control_eq(
            parse("control sources set-period 10ms").unwrap(),
            vec![
                request::source(SourceMatcher::Name(SourceNamePattern::wildcard()))
                    .set_trigger(TriggerSpec::at_interval(Duration::from_millis(10)))
                    .into(),
            ],
        );
    }

    fn assert_control_eq(cmd: Command, msg: Vec<AnyAnonymousControlRequest>) {
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
