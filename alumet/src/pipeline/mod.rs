use std::{error::Error, fmt, time::SystemTime};

use crate::{error::GenericError, metrics::{MeasurementBuffer, MeasurementAccumulator}};

pub mod runtime;
pub mod registry;
pub mod trigger;
mod threading;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    fn poll(&mut self, into: &mut MeasurementAccumulator, time: SystemTime) -> Result<(), PollError>;
}

/// Transforms the measurements.
pub trait Transform: Send {
    fn apply(&mut self, on: &mut MeasurementBuffer) -> Result<(), TransformError>;
}

/// Exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    fn write(&mut self, measurements: &MeasurementBuffer) -> Result<(), WriteError>;
}

// ====== Errors ======
#[derive(Debug)]
pub struct PollError(GenericError<PollErrorKind>);
#[derive(Debug)]
pub struct TransformError(GenericError<TransformErrorKind>);
#[derive(Debug)]
pub struct WriteError(GenericError<WriteErrorKind>);

impl PollError {
    pub fn new(kind: PollErrorKind) -> PollError {
        PollError(GenericError::new(kind))
    }

    pub fn with_description(kind: PollErrorKind, description: &str) -> PollError {
        PollError(GenericError::with_description(kind, description))
    }

    pub fn with_source<E: Error + Send + 'static>(kind: PollErrorKind, description: &str, source: E) -> PollError {
        PollError(GenericError::with_source(kind, description, source))
    }
}
impl fmt::Display for PollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub enum PollErrorKind {
    /// The source of the data could not be read.
    /// For instance, when a file contains the measurements to poll, but reading
    /// it fails, `poll()` returns an error of kind [`ReadFailed`].
    ReadFailed,
    /// The raw data could be read, but turning it into a proper measurement failed.
    /// For instance, when a file contains the measurements in some format, but reading
    /// it does not give the expected value, which causes the parsing to fail,
    /// `poll()` returns an error of kind [`ParsingFailed`].
    ParsingFailed,
    /// Polling failed in an unrecoverable way, for instance a panic has been caught,
    /// or internal data structures have been messed up. After an error of this kind is
    /// returned, `poll()` should never be called on this source.
    /// 
    /// This error is usually created by wrappers in the core of alumet, seldom by the
    /// implementation of poll().
    Unrecoverable,
}
impl fmt::Display for PollErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PollErrorKind::ReadFailed => "read failed",
            PollErrorKind::ParsingFailed => "parsing failed",
            PollErrorKind::Unrecoverable => "unrecoverable error",
        };
        f.write_str(s)
    }
}

#[derive(Debug)]
pub enum TransformErrorKind {
    /// The measurements to transform are invalid.
    UnexpectedInput,
    /// The transformation should have worked for the input measurements, but an internal error occured.
    TransformFailed,
    /// The transformation failed in an unrecoverable way, for instance a panic has been caught,
    /// or internal data structures have been messed up. After an error of this kind is
    /// returned, `apply()` should never be called on this source.
    /// 
    /// This error is usually created by wrappers in the core of alumet, seldom by the
    /// implementation of apply().
    Unrecoverable,
}
impl fmt::Display for TransformErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TransformErrorKind::UnexpectedInput => "unexpected input for transform",
            TransformErrorKind::TransformFailed => "transformation failed",
            TransformErrorKind::Unrecoverable => "unrecoverable error",
            
        };
        f.write_str(s)
    }
}
impl TransformError {
    pub fn new(kind: TransformErrorKind) -> TransformError {
        TransformError(GenericError::new(kind))
    }

    pub fn with_description(kind: TransformErrorKind, description: &str) -> TransformError {
        TransformError(GenericError::with_description(kind, description))
    }

    pub fn with_source<E: Error + Send + 'static>(kind: TransformErrorKind, description: &str, source: E) -> TransformError {
        TransformError(GenericError::with_source(kind, description, source))
    }
}
impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub enum WriteErrorKind {
    /// The data could not be written properly.
    /// For instance, the data was in the process of being sent over the network,
    /// but the connection was lost.
    WriteFailed,
    /// The data could not be transformed into a form that is appropriate for writing.
    /// For instance, the measurements lack some metadata, which causes the formatting
    /// to fail.
    FormattingFailed,
    /// Writing failed in an unrecoverable way, for instance a panic has been caught,
    /// or internal data structures have been messed up. After an error of this kind is
    /// returned, `write()` should never be called on this source.
    /// 
    /// This error is usually created by wrappers in the core of alumet, seldom by the
    /// implementation of write().
    Unrecoverable,
}
impl fmt::Display for WriteErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            WriteErrorKind::WriteFailed => "write failed",
            WriteErrorKind::FormattingFailed => "formatting failed",
            WriteErrorKind::Unrecoverable => "unrecoverable error",
        };
        f.write_str(s)
    }
}
impl WriteError {
    pub fn new(kind: WriteErrorKind) -> WriteError {
        WriteError(GenericError::new(kind))
    }

    pub fn with_description(kind: WriteErrorKind, description: &str) -> WriteError {
        WriteError(GenericError::with_description(kind, description))
    }

    pub fn with_source<E: Error + Send + 'static>(kind: WriteErrorKind, description: &str, source: E) -> WriteError {
        WriteError(GenericError::with_source(kind, description, source))
    }
}
impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
