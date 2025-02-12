use thiserror::Error;

/// An error that can occur when performing a control operation.
#[derive(Debug, Error)]
pub enum ControlError {
    #[error("Cannot send the message because the channel is full")]
    ChannelFull,
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

/// An error that can occur when sending a control message.
///
/// Unlike `ControlError`, `ControlSendError` gives back the message if the channel is full.
#[derive(Debug, Error)]
pub enum ControlSendError {
    #[error("Cannot send the message because the channel is full - {0:?}")]
    ChannelFull(super::message::ControlMessage),
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

impl ControlSendError {
    /// Removes the `ControlMessage` from the error type.
    ///
    /// The returned error is guaranteed to be `Send`, `Sync` and `Clone`-able.
    pub fn erased(self) -> ControlError {
        match self {
            ControlSendError::ChannelFull(_) => ControlError::ChannelFull,
            ControlSendError::Shutdown => ControlError::Shutdown,
        }
    }
}

impl From<ControlSendError> for ControlError {
    fn from(value: ControlSendError) -> Self {
        value.erased()
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline::util::{assert_send, assert_sync};

    use super::{ControlError, ControlSendError};

    #[test]
    fn typing() {
        assert_send::<ControlError>();
        assert_send::<ControlSendError>();
        assert_sync::<ControlError>();
        // assert_sync::<ControlSendError>(); does NOT work
    }
}
