//! Public interface for implementing transforms.

use crate::{
    measurement::{MeasurementAccumulator, MeasurementBuffer},
    metrics::registry::MetricRegistry,
};

use super::error::TransformError;

/// Transforms measurements (arbitrary transformation).
pub trait Transform: Send {
    /// Applies the transform function on the measurements.
    ///
    /// After `apply` is done, the buffer is passed to the next transform, if there is one,
    /// or to the outputs.
    ///
    /// # Transforming measurements
    /// The transform is free to manipulate the measurement buffer how it sees fit.
    /// The `apply` method can:
    /// - remove some or all measurements
    /// - add new measurements
    /// - modify the measurement points
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError>;

    /// Performs one last operation before stopping.
    ///
    /// Alumet calls `finish` before stopping.
    ///
    /// # Default implementation
    /// The default implementation does nothing.
    /// Overrides it if you need to do something before stopping, such as processing all the buffered data.
    fn finish(&mut self, ctx: &TransformContext) -> Result<(), TransformError> {
        Ok(())
    }
}

/// Shared data that can be accessed by transforms.
pub struct TransformContext<'a> {
    pub metrics: &'a MetricRegistry,
}
