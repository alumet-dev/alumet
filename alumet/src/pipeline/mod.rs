//! Asynchronous and modular measurement pipeline.

use std::fmt;

use crate::{measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp}, metrics::MetricRegistry};

pub mod runtime;
pub mod builder;
mod threading;
mod scoped;
pub mod trigger;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    /// Polls the source for new measurements.
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError>;
}

/// Transforms measurements.
pub trait Transform: Send {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), TransformError>;
}

/// Exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

/// The type of a [`Source`].
///
/// It affects how Alumet schedules the polling of the source.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceType {
    /// Nothing special. This is the right choice for most of the sources.
    Normal,
    // Blocking, // todo: how to provide this type properly?
    /// Signals that the pipeline should run the source on a thread with a
    /// high scheduling priority.
    RealtimePriority,
}

pub struct OutputContext {
    pub metrics: MetricRegistry,
}

// ====== Errors ======

/// Error which can occur during [`Source::poll`].
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

/// Error which can occur during [`Transform::apply`].
#[derive(Debug)]
pub enum TransformError {
    /// The transformation failed and cannot recover from this failure, it should not be used anymore.
    Fatal(anyhow::Error),
    /// The measurements to transform are invalid, but the `Transform` itself is fine and can be used on other measurements.
    UnexpectedInput(anyhow::Error),
}

/// Error which can occur during [`Output::write`].
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
impl<T: Into<anyhow::Error>> From<T> for TransformError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}
impl<T: Into<anyhow::Error>> From<T> for WriteError {
    fn from(value: T) -> Self {
        Self::Fatal(value.into())
    }
}

/// Adds the convenient method `error.retry_poll()`.
pub trait PollRetry<T> {
    fn retry_poll(self) -> Result<T, PollError>;
}
impl<T, E: Into<anyhow::Error>> PollRetry<T> for Result<T, E> {
    fn retry_poll(self) -> Result<T, PollError> {
        self.map_err(|e| PollError::CanRetry(e.into()))
    }
}

/// Adds the convenient method `error.retry_write()`.
pub trait WriteRetry<T> {
    fn retry_write(self) -> Result<T, WriteError>;
}
impl<T, E: Into<anyhow::Error>> WriteRetry<T> for Result<T, E> {
    fn retry_write(self) -> Result<T, WriteError> {
        self.map_err(|e| WriteError::CanRetry(e.into()))
    }
}
