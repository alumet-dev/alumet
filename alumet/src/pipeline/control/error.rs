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
    ChannelFull(super::handle::ControlMessage),
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

impl From<ControlSendError> for ControlError {
    fn from(value: ControlSendError) -> Self {
        match value {
            ControlSendError::ChannelFull(_) => ControlError::ChannelFull,
            ControlSendError::Shutdown => ControlError::Shutdown,
        }
    }
}
