use tokio::sync::oneshot;

use crate::pipeline::{
    control::{matching::OutputMatcher, messages},
    elements::output::control::{ControlMessage, TaskState},
};

use super::DirectResponseReceiver;

pub struct OutputRequestBuilder {
    matcher: OutputMatcher,
}

#[derive(Debug)]
pub struct OutputRequest {
    msg: ControlMessage,
}

/// Returns a builder that allows to build a request for controlling outputs.
pub fn output(matcher: impl Into<OutputMatcher>) -> OutputRequestBuilder {
    OutputRequestBuilder {
        matcher: matcher.into(),
    }
}

pub enum RemainingDataStrategy {
    Write,
    Ignore,
}

impl OutputRequestBuilder {
    pub fn enable(self) -> OutputRequest {
        OutputRequest {
            msg: ControlMessage {
                matcher: self.matcher,
                new_state: TaskState::Pause,
            },
        }
    }

    pub fn disable(self) -> OutputRequest {
        OutputRequest {
            msg: ControlMessage {
                matcher: self.matcher,
                new_state: TaskState::Run,
            },
        }
    }

    pub fn stop(self, remaining_strategy: RemainingDataStrategy) -> OutputRequest {
        let new_state = match remaining_strategy {
            RemainingDataStrategy::Write => TaskState::StopFinish,
            RemainingDataStrategy::Ignore => TaskState::StopNow,
        };
        OutputRequest {
            msg: ControlMessage {
                matcher: self.matcher,
                new_state,
            },
        }
    }
}

impl OutputRequest {
    fn into_body(self) -> messages::EmptyResponseBody {
        messages::EmptyResponseBody::Output(self.msg)
    }
}

impl super::AnonymousControlRequest for OutputRequest {
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
