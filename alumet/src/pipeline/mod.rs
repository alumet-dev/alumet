use std::{error::Error, fmt, time::SystemTime};

use crate::{error::GenericError, metrics::{MeasurementBuffer, MeasurementAccumulator}};

pub mod tokio;
pub mod registry;
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
        PollError(GenericError {
            kind,
            cause: None,
            description: None,
        })
    }

    pub fn with_description(kind: PollErrorKind, description: &str) -> PollError {
        PollError(GenericError {
            kind,
            cause: None,
            description: Some(description.to_owned()),
        })
    }

    pub fn with_source<E: Error + 'static>(kind: PollErrorKind, description: &str, source: E) -> PollError {
        PollError(GenericError {
            kind,
            cause: Some(Box::new(source)),
            description: Some(description.to_owned()),
        })
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
}
impl fmt::Display for PollErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PollErrorKind::ReadFailed => "read failed",
            PollErrorKind::ParsingFailed => "parsing failed",
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
}
impl fmt::Display for TransformErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TransformErrorKind::UnexpectedInput => "unexpected input for transform",
            TransformErrorKind::TransformFailed => "transformation failed",
        };
        f.write_str(s)
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
}
impl fmt::Display for WriteErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            WriteErrorKind::WriteFailed => "write failed",
            WriteErrorKind::FormattingFailed => "formatting failed",
        };
        f.write_str(s)
    }
}
