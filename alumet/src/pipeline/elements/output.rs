//! Implementation and control of output tasks.

/// Lazy creation of outputs.
pub mod builder;
pub(crate) mod control;
/// Outputs-related errors.
pub mod error;
/// Public interface for implementing outputs.
pub mod interface;
/// Functions that run outputs.
pub mod run;

pub use error::WriteError;
pub use interface::{AsyncOutputStream, BoxedAsyncOutput, Output, OutputContext};
