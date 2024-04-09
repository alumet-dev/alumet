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
use std::sync::OnceLock;

use super::measurement::{MeasurementType, WrappedMeasurementType};
use super::units::Unit;

/// The complete definition of a metric.
///
/// To register new metrics from your plugin, use
/// [`AlumetStart::create_metric`](crate::plugin::AlumetStart::create_metric)
/// or [`AlumetStart::create_metric_untyped`](crate::plugin::AlumetStart::create_metric).
/// Metrics can only be registered during the plugin startup phase.
/// 
/// See the [module docs](self).
pub struct Metric {
    /// The unique identifier of the metric in the metric registry.
    pub id: RawMetricId,
    /// The metric's unique name.
    pub name: String,
    /// A verbose description of the metric.
    pub description: String,
    /// Type of measurement, wrapped in an enum.
    pub value_type: WrappedMeasurementType,
    /// Unit that applies to all the measurements of this metric.
    pub unit: Unit,
}

/// Trait for both typed and untyped metric ids.
pub trait MetricId {
    /// Returns the unique name of the metric.
    fn name(&self) -> &str;
    /// Returns the id of the metric in the registry.
    fn untyped_id(&self) -> RawMetricId;
}

/// A registry of metrics.
///
/// New metrics are created by the plugins during their initialization.
pub struct MetricRegistry {
    pub(crate) metrics_by_id: HashMap<RawMetricId, Metric>,
    pub(crate) metrics_by_name: HashMap<String, RawMetricId>,
}

/// Global registry of metrics, to be used from the pipeline, in any thread.
///
/// The registry is NOT mutable because it is not thread-safe.
/// Metrics must be registered before the measurement pipeline starts.
pub(crate) static GLOBAL_METRICS: OnceLock<MetricRegistry> = OnceLock::new();

/// Gets a metric from the global registry.
pub(crate) fn get_metric(id: &RawMetricId) -> &'static Metric {
    MetricRegistry::global().with_id(id).unwrap_or_else(|| {
        panic!(
            "Every metric should be in the global registry, but this one was not found: {}",
            id.0
        )
    })
}

/// A metric id without a generic type information.
///
/// In general, it is preferred to use [`TypedMetricId`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct RawMetricId(pub(crate) usize);

/// A metric id with compile-time information about the type of the measured values.
///
/// It allows to check, at compile time, that the measurements of this metric
/// have a value of type `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct TypedMetricId<T: MeasurementType>(pub(crate) RawMetricId, pub(crate) PhantomData<T>);

impl MetricId for RawMetricId {
    fn name(&self) -> &str {
        let metric = get_metric(self);
        &metric.name
    }

    fn untyped_id(&self) -> RawMetricId {
        self.clone()
    }
}

impl<T: MeasurementType> MetricId for TypedMetricId<T> {
    fn untyped_id(&self) -> RawMetricId {
        self.0.clone()
    }

    fn name(&self) -> &str {
        let metric = get_metric(&self.untyped_id());
        &metric.name
    }
}

// Construction UntypedMetricId -> TypedMetricId
impl<T: MeasurementType> TypedMetricId<T> {
    pub fn try_from(untyped: RawMetricId, registry: &MetricRegistry) -> Result<Self, MetricTypeError> {
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

    /// Returns the global metric registry.
    ///
    /// This function panics the registry has not been initialized with [`MetricRegistry::init_global()`].
    pub(crate) fn global() -> &'static MetricRegistry {
        // `get` is just one atomic read, this is much cheaper than a Mutex or RwLock
        GLOBAL_METRICS
            .get()
            .expect("The MetricRegistry must be initialized before use.")
    }

    /// Sets the global metric registry.
    ///
    /// This function can only be called once.
    /// The global metric registry must be set before using a `Source`, `Transform` or `Output`, because
    /// they may call functions such as [`MetricId::name`] that use the global registry.
    pub(crate) fn init_global(registry: MetricRegistry) {
        GLOBAL_METRICS
            .set(registry)
            .unwrap_or_else(|_| panic!("The MetricRegistry can be initialized only once."));
    }

    /// Finds the metric that has the given id.
    pub fn with_id<M: MetricId>(&self, id: &M) -> Option<&Metric> {
        self.metrics_by_id.get(&id.untyped_id())
    }

    /// Finds the metric that has the given name.
    pub fn with_name(&self, name: &str) -> Option<&Metric> {
        self.metrics_by_name.get(name).and_then(|id| self.metrics_by_id.get(id))
    }

    /// The number of metrics in the registry.
    pub fn len(&self) -> usize {
        self.metrics_by_id.len()
    }

    /// An iterator on the registered metrics.
    pub fn iter(&self) -> MetricIter<'_> {
        // return new iterator
        MetricIter {
            values: self.metrics_by_id.values(),
        }
    }

    /// Registers a new metric in this registry.
    /// The `id` of `m` is ignored and replaced by a newly generated id.
    pub(crate) fn register(&mut self, mut m: Metric) -> Result<RawMetricId, MetricCreationError> {
        let name = &m.name;
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
        let id = RawMetricId(self.metrics_by_id.len());
        m.id = id;
        self.metrics_by_name.insert(name.clone(), id);
        self.metrics_by_id.insert(id, m);
        Ok(id)
    }
}

/// An iterator over the metrics of a [`MetricRegistry`].
pub struct MetricIter<'a> {
    values: std::collections::hash_map::Values<'a, RawMetricId, Metric>,
}
impl<'a> Iterator for MetricIter<'a> {
    type Item = &'a Metric;

    fn next(&mut self) -> Option<Self::Item> {
        self.values.next()
    }
}

impl<'a> IntoIterator for &'a MetricRegistry {
    type Item = &'a Metric;

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
    use crate::{
        measurement::WrappedMeasurementType,
        metrics::{Metric, RawMetricId},
        units::Unit,
    };

    use super::MetricRegistry;

    #[test]
    fn no_duplicate_metrics() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        metrics
            .register(Metric {
                id: RawMetricId(0),
                name: "metric".to_owned(),
                description: "...".to_owned(),
                value_type: WrappedMeasurementType::U64,
                unit: Unit::Watt,
            })
            .unwrap();
        metrics
            .register(Metric {
                id: RawMetricId(123),
                name: "metric".to_owned(),
                description: "abcd".to_owned(),
                value_type: WrappedMeasurementType::F64,
                unit: Unit::Volt,
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
                id: RawMetricId(0),
                name: "metric".to_owned(),
                description: "".to_owned(),
                value_type: WrappedMeasurementType::U64,
                unit: Unit::Watt,
            })
            .unwrap();
        let metric_id2 = metrics
            .register(Metric {
                id: RawMetricId(0),
                name: "metric2".to_owned(),
                description: "".to_owned(),
                value_type: WrappedMeasurementType::F64,
                unit: Unit::Watt,
            })
            .unwrap();
        assert_eq!(metrics.len(), 2);

        let metric = metrics.with_name("metric").expect("metrics.with_name failed");
        let metric2 = metrics.with_name("metric2").expect("metrics.with_name failed");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);

        let metric = metrics.with_id(&metric_id).expect("metrics.with_id failed");
        let metric2 = metrics.with_id(&metric_id2).expect("metrics.with_id failed");
        assert_eq!("metric", metric.name);
        assert_eq!("metric2", metric2.name);

        let mut names: Vec<&str> = metrics.iter().map(|m| &*m.name).collect();
        names.sort();
        assert_eq!(vec!["metric", "metric2"], names);
    }

    #[test]
    fn metric_global() {
        let mut metrics = MetricRegistry::new();
        let m = Metric {
            id: RawMetricId(usize::MAX),
            name: "metric".to_owned(),
            description: "time...".to_owned(),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Second,
        };
        let id: RawMetricId = metrics.register(m).unwrap();
        MetricRegistry::init_global(metrics);
        let metric = MetricRegistry::global().with_id(&id).unwrap();
        assert_eq!("metric", &metric.name);
        assert_eq!(WrappedMeasurementType::U64, metric.value_type);
        assert_eq!(Unit::Second, metric.unit);
        assert_eq!("time...", metric.description);
    }
}
