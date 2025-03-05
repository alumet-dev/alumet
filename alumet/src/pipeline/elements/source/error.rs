use std::fmt::{self, Display};

/// Error which can occur during [`Source::poll`](super::super::Source::poll).
#[derive(Debug)]
pub enum PollError {
    /// Polling failed and the source cannot recover from this failure, it should be stopped.
    Fatal(anyhow::Error),
    /// The error is temporary, polling again may work.
    ///
    /// You should use this kind of error when:
    /// - The source polls an external entity that you know can fail from time to time.
    /// - And the source's `poll` method can be called again and work. Pay attention to the internal state of the source.
    CanRetry(anyhow::Error),
    /// The source is no longer able to work and must be stopped, but this is expected.
    ///
    /// Use this when the object that you measure disappears in an expected way.
    /// For instance, a process can exit, which removes its associated files in the procfs,
    /// making them unreadable with a `NotFound` error.
    NormalStop,
}

impl Display for PollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PollError::Fatal(e) => write!(f, "fatal error in Source::poll: {e}"),
            PollError::CanRetry(e) => write!(f, "polling failed (but could work later): {e}"),
            PollError::NormalStop => write!(f, "the source stopped in an expected way (it's fine)"),
        }
    }
}

// Allow to convert from anyhow::Error to pipeline errors
// NOTE: this prevents PollError from implementing Error...
impl<T: Into<anyhow::Error>> From<T> for PollError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}

/// Adds the convenient method `error.retry_poll()`.
pub trait PollRetry<T> {
    fn retry_poll(self) -> Result<T, PollError>;
}

impl<T, E: Into<anyhow::Error>> PollRetry<T> for Result<T, E> {
    /// Turns this error into [`PollError::CanRetry`].
    fn retry_poll(self) -> Result<T, PollError> {
        self.map_err(|e| PollError::CanRetry(e.into()))
    }
}
