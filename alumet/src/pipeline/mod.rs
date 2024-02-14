use std::{fmt, time::SystemTime};

use crate::metrics::{MeasurementAccumulator, MeasurementBuffer};

pub mod registry;
pub mod runtime;
mod threading;
pub mod trigger;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: SystemTime) -> Result<(), PollError>;
}

/// Transforms the measurements.
pub trait Transform: Send {
    fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), TransformError>;
}

/// Exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    fn write(&mut self, measurements: &MeasurementBuffer) -> Result<(), WriteError>;
}

// ====== Errors ======

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
}

#[derive(Debug)]
pub enum TransformError {
    /// The transformation failed and cannot recover from this failure, it should not be used anymore.
    Fatal(anyhow::Error),
    /// The measurements to transform are invalid, but the `Transform` itself is fine and can be used on other measurements.
    UnexpectedInput(anyhow::Error),
}

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

impl fmt::Display for PollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PollError::Fatal(e) => write!(f, "fatal error in Source::poll: {e}"),
            PollError::CanRetry(e) => write!(f, "polling failed (but could work later): {e}"),
        }
    }
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
impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteError::Fatal(e) => write!(f, "fatal error in Output::write: {e}"),
            WriteError::CanRetry(e) => write!(f, "writing failed (but could work later): {e}"),
        }
    }
}

// Allow to convert from anyhow::Error to pipeline errors

impl<T: Into<anyhow::Error>> From<T> for PollError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}
impl From<anyhow::Error> for TransformError {
    fn from(value: anyhow::Error) -> Self {
        Self::Fatal(value)
    }
}
impl From<anyhow::Error> for WriteError {
    fn from(value: anyhow::Error) -> Self {
        Self::Fatal(value)
    }
}

// Add convenient method `error.can_retry()`
trait PollRetry {
    fn can_retry(self) -> PollError;
}
impl<T: Into<anyhow::Error>> PollRetry for T {
    fn can_retry(self) -> PollError {
        PollError::CanRetry(self.into())
    }
}
trait WriteRetry {
    fn can_retry(self) -> WriteError;
}
impl<T: Into<anyhow::Error>> WriteRetry for T {
    fn can_retry(self) -> WriteError {
        WriteError::CanRetry(self.into())
    }
}
