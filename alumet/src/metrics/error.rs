use std::fmt;

use crate::measurement::WrappedMeasurementType;

use super::duplicate::DuplicateCriteria;

/// Error which can occur when creating a new metric.
///
/// This error is returned when the metric cannot be registered because of a conflict,
/// that is, another metric with the same name has already been registered.
#[derive(Debug, Clone)]
pub struct MetricCreationError {
    pub name: String,
    pub criteria: DuplicateCriteria,
}

impl std::error::Error for MetricCreationError {}

impl fmt::Display for MetricCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "This metric has already been registered: {}", self.name)
    }
}

/// Error which can occur when converting a [`RawMetricId`] to a [`TypedMetricId`].
///
/// This error occurs if the expected type does not match the measurement type of the raw metric.
#[derive(Debug)]
pub struct MetricTypeError {
    pub(super) expected: WrappedMeasurementType,
    pub(super) actual: WrappedMeasurementType,
}
impl std::error::Error for MetricTypeError {}
impl std::fmt::Display for MetricTypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Incompatible metric type: expected {} but was {}",
            self.expected, self.actual
        )
    }
}
