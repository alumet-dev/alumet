use std::{future::Future, pin::Pin};

use crate::{measurement::MeasurementBuffer, metrics::registry::MetricRegistry};

use super::error::WriteError;

/// A blocking output that exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

/// An asynchronous stream of measurements, to be used by an asynchronous output.
pub struct AsyncOutputStream(
    pub Pin<Box<dyn futures::Stream<Item = Result<MeasurementBuffer, StreamRecvError>> + Send>>,
); // TODO make opaque?

pub use crate::pipeline::util::channel::StreamRecvError;
pub type BoxedAsyncOutput = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Shared data that can be accessed by outputs.
pub struct OutputContext<'a> {
    pub metrics: &'a MetricRegistry,
}
