use crate::pipeline::{
    control::matching::SourceMatcher,
    elements::source::{
        control::{ConfigureCommand, ConfigureMessage, ControlMessage},
        trigger::TriggerSpec,
    },
};

pub struct SourceRequestBuilder {
    matcher: SourceMatcher,
}

#[derive(Debug)]
pub struct SourceRequest {
    msg: ControlMessage,
}

pub fn source(matcher: impl Into<SourceMatcher>) -> SourceRequestBuilder {
    SourceRequestBuilder {
        matcher: matcher.into(),
    }
}

impl SourceRequestBuilder {
    pub fn set_trigger(self, spec: TriggerSpec) -> SourceRequest {
        SourceRequest {
            msg: ControlMessage::Configure(ConfigureMessage {
                matcher: self.matcher,
                command: ConfigureCommand::SetTrigger(spec),
            }),
        }
    }

    pub fn trigger_now(self) -> SourceRequest {
        SourceRequest {
            msg: ControlMessage::TriggerManually(crate::pipeline::elements::source::control::TriggerMessage {
                matcher: self.matcher,
            }),
        }
    }

    pub fn stop(self) -> SourceRequest {
        SourceRequest {
            msg: ControlMessage::Configure(ConfigureMessage {
                matcher: self.matcher,
                command: ConfigureCommand::Stop,
            }),
        }
    }

    pub fn disable(self) -> SourceRequest {
        SourceRequest {
            msg: ControlMessage::Configure(ConfigureMessage {
                matcher: self.matcher,
                command: ConfigureCommand::Pause,
            }),
        }
    }

    pub fn enable(self) -> SourceRequest {
        SourceRequest {
            msg: ControlMessage::Configure(ConfigureMessage {
                matcher: self.matcher,
                command: ConfigureCommand::Resume,
            }),
        }
    }
}

impl super::ControlRequest for SourceRequest {
    fn serialize(self) -> super::ControlRequestBody {
        super::ControlRequestBody::Source(self.msg)
    }
}
