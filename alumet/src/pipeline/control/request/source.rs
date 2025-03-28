use tokio::sync::oneshot;

use crate::pipeline::{
    control::{matching::SourceMatcher, messages},
    elements::source::{
        control::{ConfigureCommand, ConfigureMessage, ControlMessage},
        trigger::TriggerSpec,
    },
};

use super::DirectResponseReceiver;

pub struct SourceRequestBuilder {
    matcher: SourceMatcher,
}

#[derive(Debug)]
pub struct SourceRequest {
    msg: ControlMessage,
}

/// Returns a builder that allows to build a request for controlling sources.
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

impl SourceRequest {
    fn into_body(self) -> messages::EmptyResponseBody {
        messages::EmptyResponseBody::Source(self.msg)
    }
}

impl super::AnonymousControlRequest for SourceRequest {
    type OkResponse = ();
    type Receiver = DirectResponseReceiver<()>;

    fn serialize(self) -> messages::ControlRequest {
        messages::ControlRequest::NoResult(messages::RequestMessage {
            response_tx: None,
            body: self.into_body(),
        })
    }

    fn serialize_with_response(self) -> (messages::ControlRequest, Self::Receiver) {
        let (tx, rx) = oneshot::channel();
        let req = messages::ControlRequest::NoResult(messages::RequestMessage {
            response_tx: Some(tx),
            body: self.into_body(),
        });
        (req, DirectResponseReceiver(rx))
    }
}
