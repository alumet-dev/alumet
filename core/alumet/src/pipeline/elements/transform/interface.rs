//! Public interface for implementing transforms.

use crate::{measurement::MeasurementBuffer, metrics::registry::MetricRegistry};

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
}

/// Shared data that can be accessed by transforms.
pub struct TransformContext<'a> {
    pub metrics: &'a MetricRegistry,
}
