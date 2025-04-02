use tokio::sync::oneshot;

use crate::pipeline::{
    control::{matching::TransformMatcher, messages},
    elements::transform::control::{ControlMessage, TaskState},
};

use super::DirectResponseReceiver;

pub struct TransformRequestBuilder {
    matcher: TransformMatcher,
}

#[derive(Debug)]
pub struct TransformRequest {
    msg: ControlMessage,
}

/// Returns a builder that allows to build a request for controlling transforms.
pub fn transform(matcher: impl Into<TransformMatcher>) -> TransformRequestBuilder {
    TransformRequestBuilder {
        matcher: matcher.into(),
    }
}

impl TransformRequestBuilder {
    pub fn disable(self) -> TransformRequest {
        TransformRequest {
            msg: ControlMessage {
                matcher: self.matcher,
                new_state: TaskState::Disabled,
            },
        }
    }

    pub fn enable(self) -> TransformRequest {
        TransformRequest {
            msg: ControlMessage {
                matcher: self.matcher,
                new_state: TaskState::Enabled,
            },
        }
    }
}

impl TransformRequest {
    fn into_body(self) -> messages::EmptyResponseBody {
        messages::EmptyResponseBody::Single(messages::SpecificBody::Transform(self.msg))
    }
}

impl super::AnonymousControlRequest for TransformRequest {
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
