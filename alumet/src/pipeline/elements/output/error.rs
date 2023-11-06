use std::fmt;

/// Error which can occur during [`Output::write`](super::super::Output::write).
#[derive(Debug)]
pub enum WriteError {
    /// The measurements could not be written properly, and the output cannot be used anymore.
    ///
    /// For instance, a panic may have been caught, or internal data structures may have been messed up.
    Fatal(anyhow::Error),
    /// The error is temporary, writing again may work.
    ///
    /// You should use this kind of error when:
    /// - The output communicates with an external entity that you know can fail from time to time.
    /// - And the output's `write` method can be called again and work. Pay attention to the internal state of the output.
    CanRetry(anyhow::Error),
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Fatal(e) => write!(f, "fatal error in Output::write: {e}"),
            WriteError::CanRetry(e) => write!(f, "writing failed (but could work later): {e}"),
        }
    }
}

impl<T: Into<anyhow::Error>> From<T> for WriteError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}

/// Adds the convenient method `error.retry_write()`.
pub trait WriteRetry<T> {
    fn retry_write(self) -> Result<T, WriteError>;
}
impl<T, E: Into<anyhow::Error>> WriteRetry<T> for Result<T, E> {
    /// Turns this error into [`WriteError::CanRetry`].
    fn retry_write(self) -> Result<T, WriteError> {
        self.map_err(|e| WriteError::CanRetry(e.into()))
    }
}
