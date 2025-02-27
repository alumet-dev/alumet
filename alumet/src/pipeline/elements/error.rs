use thiserror::Error;

pub use crate::pipeline::elements::output::error::WriteError;
pub use crate::pipeline::elements::source::error::PollError;
pub use crate::pipeline::elements::transform::error::TransformError;
// TODO remove the above aliases

use crate::pipeline::naming::ElementName;

/// A pipeline element has stopped because of an error.
#[derive(Debug, Error)]
#[error("{element} stopped because of a fatal error")]
pub(crate) struct FatalElementError {
    pub element: ElementName,
    #[source]
    error: anyhow::Error,
}

impl FatalElementError {
    pub(crate) fn new(element: ElementName, error: anyhow::Error) -> Self {
        Self { element, error }
    }
}
