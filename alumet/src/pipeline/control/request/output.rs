use crate::pipeline::{
    control::matching::OutputMatcher,
    elements::output::control::{ControlMessage, TaskState},
};

pub struct OutputRequestBuilder {
    matcher: OutputMatcher,
}

#[derive(Debug)]
pub struct OutputRequest {
    msg: ControlMessage,
}

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

impl super::ControlRequest for OutputRequest {
    fn serialize(self) -> super::ControlRequestBody {
        super::ControlRequestBody::Output(self.msg)
    }
}
