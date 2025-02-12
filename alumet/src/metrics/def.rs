//! Definition of metrics.
//!
//! A metric is defined by the following data:
//! - a unique name
//! - a description
//! - a type of measured value
//! - a measurement unit
//!
//! This information is stored in the [`Metric`] struct.
//!
//! # Metric identifiers
//! In addition to this definition, Alumet assigns a unique id to each metric,
//! which is used to refer to a metric from anywhere in the program.
//! This id can exist in two forms:
//! - A [`RawMetricId`], which is the most basic form of metric id, and does not offer any compile-time type safety.
//! - A [`TypedMetricId`], which carries some information about the metric and allows
//! to check the type of the measured values at compile time. It is a zero-cost wrapper around a `RawMetricId`.
//!
//! # Registering new metrics
//! Metrics can only be registered during the plugin startup phase.
//! To register new metrics, use [`AlumetPluginStart::create_metric`](crate::plugin::AlumetPluginStart::create_metric)
//! or [`AlumetPluginStart::create_metric_untyped`](crate::plugin::AlumetPluginStart::create_metric).
//! You can then pass the id around.
//!
//! # Example
//!
//! ```no_run
//! use alumet::plugin::AlumetPluginStart;
//! use alumet::metrics::TypedMetricId;
//! use alumet::units::Unit;
//!
//! # fn start(alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
//! let my_metric: TypedMetricId<u64> = alumet.create_metric::<u64>(
//!     "cpu_voltage",
//!     Unit::Volt,
//!     "Voltage of the CPU socket, measured by the internal shunt."
//! )?;
//! # Ok(())
//! # }
//! ```

use std::marker::PhantomData;

use crate::measurement::{MeasurementType, WrappedMeasurementType};
use crate::units::PrefixedUnit;

use super::error::MetricTypeError;
use super::registry::MetricRegistry;

/// The complete definition of a metric (without its id).
///
/// To register new metrics from your plugin, use
/// [`AlumetPluginStart::create_metric`](crate::plugin::AlumetPluginStart::create_metric)
/// or [`AlumetPluginStart::create_metric_untyped`](crate::plugin::AlumetPluginStart::create_metric).
///
/// See the [module docs](self).
#[derive(Debug, Clone)]
pub struct Metric {
    /// The metric's unique name.
    pub name: String,
    /// A verbose description of the metric.
    pub description: String,
    /// Type of measurement, wrapped in an enum.
    pub value_type: WrappedMeasurementType,
    /// Unit that applies to all the measurements of this metric.
    pub unit: PrefixedUnit,
}

/// Trait for both typed and untyped metric ids.
pub trait MetricId {
    /// Returns the id of the metric in the registry.
    fn untyped_id(&self) -> RawMetricId;
}

/// A metric id without a generic type information.
///
/// In general, it is preferred to use [`TypedMetricId`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct RawMetricId(pub(crate) usize);

impl RawMetricId {
    pub fn as_u64(&self) -> u64 {
        self.0 as u64
    }

    pub fn from_u64(id: u64) -> RawMetricId {
        RawMetricId(id as _)
    }
}

/// A metric id with compile-time information about the type of the measured values.
///
/// It allows to check, at compile time, that the measurements of this metric
/// have a value of type `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct TypedMetricId<T: MeasurementType>(pub(crate) RawMetricId, pub(crate) PhantomData<T>);

impl MetricId for RawMetricId {
    fn untyped_id(&self) -> RawMetricId {
        *self
    }
}

impl<T: MeasurementType> MetricId for TypedMetricId<T> {
    fn untyped_id(&self) -> RawMetricId {
        self.0
    }
}

// Construction UntypedMetricId -> TypedMetricId
impl<T: MeasurementType> TypedMetricId<T> {
    pub fn try_from(untyped: RawMetricId, registry: &MetricRegistry) -> Result<Self, MetricTypeError> {
        let expected_type = T::wrapped_type();
        let actual_type = registry
            .by_id(&untyped)
            .expect("the untyped metric should exist in the registry")
            .value_type
            .clone();
        if expected_type != actual_type {
            Err(MetricTypeError {
                expected: expected_type,
                actual: actual_type,
            })
        } else {
            Ok(TypedMetricId(untyped, PhantomData))
        }
    }
}
