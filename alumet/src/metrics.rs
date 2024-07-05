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
//! ## Metric identifiers
//! In addition to this definition, Alumet assigns a unique id to each metric,
//! which is used to refer to a metric from anywhere in the program.
//! This id can exist in two forms:
//! - A [`RawMetricId`], which is the most basic form of metric id, and does not offer any compile-time type safety.
//! - A [`TypedMetricId`], which carries some information about the metric and allows
//! to check the type of the measured values at compile time. It is a zero-cost wrapper around a `RawMetricId`.
//!
//! ## Registering new metrics
//! Metrics can only be registered during the plugin startup phase.
//! To register new metrics, use [`AlumetStart::create_metric`](crate::plugin::AlumetStart::create_metric)
//! or [`AlumetStart::create_metric_untyped`](crate::plugin::AlumetStart::create_metric).
//! You can then pass the id around.
//!
//! ### Example
//!
//! ```no_run
//! use alumet::plugin::AlumetStart;
//! use alumet::metrics::TypedMetricId;
//! use alumet::units::Unit;
//!
//! # fn start(alumet: &mut AlumetStart) -> anyhow::Result<()> {
//! let my_metric: TypedMetricId<u64> = alumet.create_metric::<u64>(
//!     "cpu_voltage",
//!     Unit::Volt,
//!     "Voltage of the CPU socket, measured by the internal shunt."
//! )?;
//! # Ok(())
//! # }
//! ```

use core::fmt;
use std::collections::HashMap;
use std::error::Error;
use std::marker::PhantomData;

use super::measurement::{MeasurementType, WrappedMeasurementType};
use super::units::PrefixedUnit;

/// The complete definition of a metric.
///
/// To register new metrics from your plugin, use
/// [`AlumetStart::create_metric`](crate::plugin::AlumetStart::create_metric)
/// or [`AlumetStart::create_metric_untyped`](crate::plugin::AlumetStart::create_metric).
/// Metrics can only be registered during the plugin startup phase.
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

/// A registry of metrics.
///
/// New metrics are created by the plugins during their initialization.
#[derive(Clone)]
pub struct MetricRegistry {
    pub(crate) metrics_by_id: HashMap<RawMetricId, Metric>,
    pub(crate) metrics_by_name: HashMap<String, RawMetricId>,
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

/// Error which can occur when converting a [`RawMetricId`] to a [`TypedMetricId`].
///
/// This error occurs if the expected type does not match the measurement type of the raw metric.
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

impl MetricRegistry {
    /// Creates a new registry, but does not make it "global" yet.
    pub(crate) fn new() -> MetricRegistry {
        MetricRegistry {
            metrics_by_id: HashMap::new(),
            metrics_by_name: HashMap::new(),
        }
    }

    /// Finds the metric that has the given id.
    pub fn by_id<M: MetricId>(&self, id: &M) -> Option<&Metric> {
        self.metrics_by_id.get(&id.untyped_id())
    }

    /// Finds the metric that has the given name.
    pub fn by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        self.metrics_by_name
            .get(name)
            .and_then(|id| self.metrics_by_id.get(id).map(|m| (*id, m)))
    }

    /// The number of metrics in the registry.
    pub fn len(&self) -> usize {
        self.metrics_by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metrics_by_id.is_empty()
    }

    /// An iterator on the registered metrics.
    pub fn iter(&self) -> MetricIter<'_> {
        // return new iterator
        MetricIter {
            entries: self.metrics_by_id.iter(),
        }
    }

    /// Registers a new metric in this registry.
    ///
    /// A new id is generated and returned.
    pub(crate) fn register(&mut self, m: Metric) -> Result<RawMetricId, MetricCreationError> {
        let name = &m.name;
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
        let id = RawMetricId(self.metrics_by_name.len());
        self.metrics_by_name.insert(name.clone(), id);
        self.metrics_by_id.insert(id, m);
        Ok(id)
    }

    pub(crate) fn extend(&mut self, metrics: Vec<Metric>) -> Result<Vec<RawMetricId>, MetricCreationError> {
        metrics.into_iter().map(|m| self.register(m)).collect()
    }

    fn deduplicated_name(&self, requested_name: &str, resolution_suffix: &str) -> String {
        if let Some(_conflict) = self.metrics_by_name.get(requested_name) {
            let mut name = format!("{requested_name}_{resolution_suffix}");
            let mut dedup = 0;
            while self.metrics_by_name.get(&name).is_some() {
                dedup += 1;
                name = format!("{requested_name}_{resolution_suffix}__{dedup}");
            }
            name
        } else {
            requested_name.to_owned()
        }
    }

    #[allow(dead_code)]
    pub(crate) fn register_infallible(&mut self, mut m: Metric, dedup_suffix: &str) -> RawMetricId {
        m.name = self.deduplicated_name(&m.name, dedup_suffix);
        self.register(m).unwrap()
    }

    pub(crate) fn extend_infallible(&mut self, metrics: Vec<Metric>, dedup_suffix: &str) -> Vec<RawMetricId> {
        self.metrics_by_name.reserve(metrics.len());
        self.metrics_by_id.reserve(metrics.len());
        let base_id = self.len();
        metrics
            .into_iter()
            .enumerate()
            .map(|(i, mut metric)| {
                metric.name = self.deduplicated_name(&metric.name, dedup_suffix);
                let id = RawMetricId(base_id + i);
                self.metrics_by_name.insert(metric.name.clone(), id);
                self.metrics_by_id.insert(id, metric);
                id
            })
            .collect()
    }
}

/// An iterator over the metrics of a [`MetricRegistry`].
pub struct MetricIter<'a> {
    entries: std::collections::hash_map::Iter<'a, RawMetricId, Metric>,
}
impl<'a> Iterator for MetricIter<'a> {
    type Item = (&'a RawMetricId, &'a Metric);

    fn next(&mut self) -> Option<Self::Item> {
        self.entries.next()
    }
}

impl<'a> IntoIterator for &'a MetricRegistry {
    type Item = (&'a RawMetricId, &'a Metric);

    type IntoIter = MetricIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

// ====== Errors ======

/// Error which can occur when creating a new metric.
///
/// This error is returned when the metric cannot be registered because of a conflict,
/// that is, another metric with the same name has already been registered.
#[derive(Debug)]
pub struct MetricCreationError {
    pub key: String,
}

impl MetricCreationError {
    pub fn new(metric_name: String) -> MetricCreationError {
        MetricCreationError { key: metric_name }
    }
}

impl Error for MetricCreationError {}

impl fmt::Display for MetricCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "This metric has already been registered: {}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use crate::{measurement::WrappedMeasurementType, metrics::Metric, units::Unit};

    use super::MetricRegistry;

    #[test]
    fn no_duplicate_metrics() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        metrics
            .register(Metric {
                name: "metric".to_owned(),
                description: "...".to_owned(),
                value_type: WrappedMeasurementType::U64,
                unit: Unit::Watt.into(),
            })
            .unwrap();
        metrics
            .register(Metric {
                name: "metric".to_owned(),
                description: "abcd".to_owned(),
                value_type: WrappedMeasurementType::F64,
                unit: Unit::Volt.into(),
            })
            .unwrap_err();
        assert_eq!(metrics.len(), 1);
    }

    #[test]
    fn metric_registry() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        let metric_id = metrics
            .register(Metric {
                name: "metric".to_owned(),
                description: "".to_owned(),
                value_type: WrappedMeasurementType::U64,
                unit: Unit::Watt.into(),
            })
            .unwrap();
        let metric_id2 = metrics
            .register(Metric {
                name: "metric2".to_owned(),
                description: "".to_owned(),
                value_type: WrappedMeasurementType::F64,
                unit: Unit::Watt.into(),
            })
            .unwrap();
        assert_eq!(metrics.len(), 2);

        let (id, metric) = metrics.by_name("metric").expect("metrics.with_name failed");
        let (id2, metric2) = metrics.by_name("metric2").expect("metrics.with_name failed");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);

        let metric = metrics.by_id(&metric_id).expect("metrics.with_id failed");
        let metric2 = metrics.by_id(&metric_id2).expect("metrics.with_id failed");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);

        let mut names: Vec<&str> = metrics.iter().map(|m| &*m.1.name).collect();
        names.sort();
        assert_eq!(vec!["metric", "metric2"], names);
    }
}
