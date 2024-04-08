use core::fmt;
use std::marker::PhantomData;

use super::measurement::{MeasurementType, WrappedMeasurementType};
use super::pipeline::registry::MetricRegistry;
use super::units::Unit;

/// All information about a metric.
pub struct Metric {
    pub id: UntypedMetricId,
    pub name: String,
    pub description: String,
    pub value_type: WrappedMeasurementType,
    pub unit: Unit,
}

pub trait MetricId {
    fn name(&self) -> &str;
    fn untyped_id(&self) -> UntypedMetricId;
}
pub(crate) fn get_metric<M: MetricId>(metric: &M) -> &'static Metric {
    MetricRegistry::global()
        .with_id(&metric.untyped_id())
        .unwrap_or_else(|| {
            panic!(
                "Every metric should be in the global registry, but this one was not found: {}",
                metric.untyped_id().0
            )
        })
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct UntypedMetricId(pub(crate) usize);

impl MetricId for UntypedMetricId {
    fn name(&self) -> &str {
        let metric = get_metric(self);
        &metric.name
    }

    fn untyped_id(&self) -> UntypedMetricId {
        self.clone()
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct TypedMetricId<T: MeasurementType>(pub(crate) usize, PhantomData<T>);

impl<T: MeasurementType> MetricId for TypedMetricId<T> {
    fn untyped_id(&self) -> UntypedMetricId {
        UntypedMetricId(self.0)
    }

    fn name(&self) -> &str {
        let metric = get_metric(self);
        &metric.name
    }
}
// Manually implement Copy because Type is not copy, but we still want TypedMetricId to be Copy
impl<T: MeasurementType> Copy for TypedMetricId<T> {}
impl<T: MeasurementType> Clone for TypedMetricId<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

// Construction UntypedMetricId -> TypedMetricId
impl<T: MeasurementType> TypedMetricId<T> {
    pub fn try_from(untyped: UntypedMetricId, registry: &MetricRegistry) -> Result<Self, MetricTypeError> {
        let expected_type = T::wrapped_type();
        let actual_type = registry
            .with_id(&untyped)
            .expect("the untyped metric should exist in the registry")
            .value_type
            .clone();
        if expected_type != actual_type {
            Err(MetricTypeError {
                expected: expected_type,
                actual: actual_type,
            })
        } else {
            Ok(TypedMetricId(untyped.0, PhantomData))
        }
    }
}

#[derive(Debug)]
pub struct MetricTypeError {
    expected: WrappedMeasurementType,
    actual: WrappedMeasurementType,
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
