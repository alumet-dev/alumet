use std::fmt;

/// Error which can occur during [`Transform::apply`](super::super::Transform::apply).
#[derive(Debug)]
pub enum TransformError {
    /// The transformation failed and cannot recover from this failure, it should not be used anymore.
    Fatal(anyhow::Error),
    /// The measurements to transform are invalid, but the `Transform` itself is fine and can be used on other measurements.
    UnexpectedInput(anyhow::Error),
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::Fatal(e) => write!(f, "fatal error in Transform::apply: {e}"),
            TransformError::UnexpectedInput(e) => write!(
                f,
                "unexpected input for transform, is the plugin properly configured? {e}"
            ),
        }
    }
}

impl<T: Into<anyhow::Error>> From<T> for TransformError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}
