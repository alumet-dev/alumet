use crate::pipeline::{
    control::matching::TransformMatcher,
    elements::transform::control::{ControlMessage, TaskState},
};

pub struct TransformRequestBuilder {
    matcher: TransformMatcher,
}

#[derive(Debug)]
pub struct TransformRequest {
    msg: ControlMessage,
}

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

impl super::ControlRequest for TransformRequest {
    fn serialize(self) -> super::ControlRequestBody {
        super::ControlRequestBody::Transform(self.msg)
    }
}
