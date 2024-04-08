use core::fmt;
use std::collections::HashMap;
use std::error::Error;
use std::marker::PhantomData;
use std::sync::OnceLock;

use super::measurement::{MeasurementType, WrappedMeasurementType};
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

/// A registry of metrics.
/// 
/// New metrics are created by the plugins during their initialization.
/// To do so, they use the methods provided by [`crate::plugin::AlumetStart`], not `MetricRegistry`.
pub struct MetricRegistry {
    pub(crate) metrics_by_id: HashMap<UntypedMetricId, Metric>,
    pub(crate) metrics_by_name: HashMap<String, UntypedMetricId>,
}

/// Global registry of metrics, to be used from the pipeline, in any thread.
pub(crate) static GLOBAL_METRICS: OnceLock<MetricRegistry> = OnceLock::new();

impl MetricRegistry {
    /// Creates a new registry, but does not make it "global" yet.
    pub fn new() -> MetricRegistry {
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
    pub(crate) fn init_global(reg: MetricRegistry) {
        GLOBAL_METRICS
            .set(reg)
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

    /// Creates a new metric and registers it in this registry.
    /// For internal use only to keep the registry's internal structure private.
    pub(crate) fn create_metric(
        &mut self,
        name: &str,
        value_type: WrappedMeasurementType,
        unit: Unit,
        description: &str,
    ) -> Result<UntypedMetricId, MetricCreationError> {
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
        let id = UntypedMetricId(self.metrics_by_id.len());
        let m = Metric {
            id,
            name: String::from(name),
            description: String::from(description),
            value_type,
            unit,
        };
        self.metrics_by_name.insert(String::from(name), id);
        self.metrics_by_id.insert(id, m);
        Ok(id)
    }
}

pub struct MetricIter<'a> {
    values: std::collections::hash_map::Values<'a, UntypedMetricId, Metric>,
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
    use crate::{measurement::WrappedMeasurementType, units::Unit};

    use super::MetricRegistry;

    #[test]
    fn no_duplicate_metrics() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        metrics.create_metric("metric", WrappedMeasurementType::U64, Unit::Watt, "...").unwrap();
        metrics.create_metric("metric", WrappedMeasurementType::U64, Unit::Watt, "...").unwrap_err();
        metrics.create_metric("metric", WrappedMeasurementType::F64, Unit::Unity, "").unwrap_err();
        assert_eq!(metrics.len(), 1);
    }
    
    #[test]
    fn metric_registry() {
        let mut metrics = MetricRegistry::new();
        assert_eq!(metrics.len(), 0);
        let metric_id = metrics.create_metric("metric", WrappedMeasurementType::U64, Unit::Watt, "...").unwrap();
        let metric_id2 = metrics.create_metric("metric2", WrappedMeasurementType::F64, Unit::Joule, "...").unwrap();
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
        let id = metrics.create_metric("metric", WrappedMeasurementType::U64, Unit::Second, "time").unwrap();
        
        MetricRegistry::init_global(metrics);
        let metrics = MetricRegistry::global();
        let metric = metrics.with_id(&id).unwrap();
        assert_eq!("metric", &metric.name);
        assert_eq!(WrappedMeasurementType::U64, metric.value_type);
        assert_eq!(Unit::Second, metric.unit);
        assert_eq!("time", metric.description);
    }
}
