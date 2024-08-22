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
//! To register new metrics, use [`AlumetPluginStart::create_metric`](crate::plugin::AlumetPluginStart::create_metric)
//! or [`AlumetPluginStart::create_metric_untyped`](crate::plugin::AlumetPluginStart::create_metric).
//! You can then pass the id around.
//!
//! ### Example
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

use core::fmt;
use std::collections::HashMap;
use std::error::Error;
use std::marker::PhantomData;

use super::measurement::{MeasurementType, WrappedMeasurementType};
use super::units::PrefixedUnit;

/// The complete definition of a metric (except its id).
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

    /// Generates a new id for a metric and insert it in the registry data structures.
    ///
    /// NOTE: the caller must ensure that the name of the metric is unique.
    fn register_new(&mut self, m: Metric) -> RawMetricId {
        let id = RawMetricId(self.metrics_by_name.len());

        let prev = self.metrics_by_name.insert(m.name.clone(), id);
        debug_assert!(prev.is_none(), "duplicate metric name {}", m.name);

        let prev = self.metrics_by_id.insert(id, m);
        debug_assert!(prev.is_none(), "duplicate metric id {}", id.0);

        id
    }

    /// Registers a new metric in this registry.
    ///
    /// A new id is generated and returned.
    ///
    /// ## Duplicates
    /// Metric names are intended to be unique.
    /// If a metric with the same name has already been registered, `register` reurns an error.
    pub(crate) fn register(&mut self, m: Metric) -> Result<RawMetricId, MetricCreationError> {
        let name = &m.name;
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
        let id = self.register_new(m);
        Ok(id)
    }

    /// Registers multiple metrics.
    ///
    /// For each metric, the registration may fail if a metric with the same name already exists.
    /// See [`register()`].
    pub(crate) fn extend(&mut self, metrics: Vec<Metric>) -> Vec<Result<RawMetricId, MetricCreationError>> {
        self.metrics_by_name.reserve(metrics.len());
        self.metrics_by_id.reserve(metrics.len());
        metrics.into_iter().map(|m| self.register(m)).collect()
    }

    /// Registers a new metric in this registry.
    ///
    /// A new id is generated and returned.
    ///
    /// ## Duplicates
    /// Contrary to [`register()`], `register_infallible` does not return an error if a metric with the
    /// same name as `m` already exists in the registry.
    ///
    /// Instead, it:
    /// 1. Checks whether `m` and the conflicting metric are "equal" (same name, same unit, same type of value).
    /// 2. If `m` is different, `register_infallible` uses the `dedup_suffix` to generate a new, unique name for `m`,
    /// and registers it under that name.
    pub(crate) fn register_infallible(&mut self, m: Metric, dedup_suffix: &str) -> RawMetricId {
        fn resolve_conflict(reg: &mut MetricRegistry, mut metric: Metric, dedup_suffix: &str) -> RawMetricId {
            use std::fmt::Write;

            // Information needed to compare metrics.
            let unit = metric.unit.clone();
            let value_type = metric.value_type.clone();

            // The metric name is modified by this function.
            let mut buf = &mut metric.name;

            // First try: simply append the suffix with an underscore
            write!(&mut buf, "_{dedup_suffix}").expect("dedup_suffix should be writable to metric name");
            match reg.by_name(buf) {
                Some((id, existing)) if existing.unit == unit && existing.value_type == value_type => id,
                Some((_id, _conflict)) => {
                    // Second try: append "_2"
                    buf.push_str("_2");
                    let len_without_n = buf.len() - 1;
                    let mut n = 2;
                    let mut existing = reg.by_name(buf);
                    while existing.is_some() {
                        let (id, other) = existing.unwrap();
                        if other.unit == unit && other.value_type == value_type {
                            // identical to the existing metric, stop here
                            return id;
                        }
                        // n-th try: replace "2" by "{n}"
                        buf.truncate(len_without_n);
                        write!(&mut buf, "{n}").expect("n should be writable to string");
                        n += 1;
                        existing = reg.by_name(buf);
                    }
                    reg.register_new(metric)
                }
                None => reg.register_new(metric),
            }
        }

        let name = &m.name;
        if let Some(conflict_id) = self.metrics_by_name.get(name) {
            let conflict = &self.metrics_by_id[conflict_id];
            if conflict.unit == m.unit && conflict.value_type == m.value_type {
                // If the conflicting metric is the same, it's ok.
                *conflict_id
            } else {
                // If it's different, create a new metric with a slightly different name, using the suffix.
                resolve_conflict(self, m, dedup_suffix)
            }
        } else {
            self.register_new(m)
        }
    }

    /// Registers multiple metrics, resolving conflicts by deduplicating names.
    ///
    /// The registration cannot fail. See [`register_infallible()`].
    pub(crate) fn extend_infallible(&mut self, metrics: Vec<Metric>, dedup_suffix: &str) -> Vec<RawMetricId> {
        self.metrics_by_name.reserve(metrics.len());
        self.metrics_by_id.reserve(metrics.len());
        metrics
            .into_iter()
            .map(|m| self.register_infallible(m, dedup_suffix))
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
#[derive(Debug, Clone)]
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
            .unwrap_err(); // error is expected
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

        let (_id, metric) = metrics.by_name("metric").expect("metrics.with_name failed");
        let (_id2, metric2) = metrics.by_name("metric2").expect("metrics.with_name failed");
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

    #[test]
    fn register_infallible() {
        {
            let mut metrics = MetricRegistry::new();
            assert_eq!(metrics.len(), 0);

            // first registration
            let id1 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");

            // register again with the same metric, the metric should not change
            let id1_bis = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "...".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Watt.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 1);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(id1, id1_bis);

            // register another metric with the same name, it should be deduplicated
            let id2 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "abcd".to_owned(),
                    value_type: WrappedMeasurementType::F64,
                    unit: Unit::Volt.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 2);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_ne!(id2, id1);

            // register another one, which is actually the same as `metric_suffix`
            let id2_bis = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "abcd".to_owned(),
                    value_type: WrappedMeasurementType::F64,
                    unit: Unit::Volt.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 2);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(id2, id2_bis);

            // register yet another one, which is different
            let id3 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "xyz".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Volt.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 3);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(metrics.by_name("metric_suffix_2").unwrap().1.name, "metric_suffix_2");
            assert_ne!(id3, id2);
            assert_ne!(id3, id1);

            // register YET another one, which is different
            let id4 = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "not the same".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Second.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 4);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(metrics.by_name("metric_suffix_2").unwrap().1.name, "metric_suffix_2");
            assert_eq!(metrics.by_name("metric_suffix_3").unwrap().1.name, "metric_suffix_3");
            assert_ne!(id4, id3);
            assert_ne!(id4, id2);
            assert_ne!(id4, id1);
            
            // and the same as metric_suffix_3
            let id4_bis = metrics.register_infallible(
                Metric {
                    name: "metric".to_owned(),
                    description: "not the same".to_owned(),
                    value_type: WrappedMeasurementType::U64,
                    unit: Unit::Second.into(),
                },
                "suffix",
            );
            assert_eq!(metrics.len(), 4);
            assert_eq!(metrics.by_name("metric").unwrap().1.name, "metric");
            assert_eq!(metrics.by_name("metric_suffix").unwrap().1.name, "metric_suffix");
            assert_eq!(metrics.by_name("metric_suffix_2").unwrap().1.name, "metric_suffix_2");
            assert_eq!(metrics.by_name("metric_suffix_3").unwrap().1.name, "metric_suffix_3");
            assert_eq!(id4_bis, id4);
        }
    }
}
