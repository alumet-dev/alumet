use std::{fmt, sync::OnceLock};
use std::error::Error;
use std::collections::HashMap;

use crate::{
    pipeline::{Output, Source, Transform}, units::Unit
};

use crate::{metrics::{Metric, MetricId, MeasurementType}, pipeline};

pub struct MetricRegistry {
    pub(crate) metrics_by_id: HashMap<MetricId, Metric>,
    pub(crate) metrics_by_name: HashMap<String, MetricId>,
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
        GLOBAL_METRICS.get().expect("The MetricRegistry must be initialized before use.")
    }

    /// Sets the global metric registry.
    /// 
    /// This function can only be called once.
    /// The global metric registry must be set before using a `Source`, `Transform` or `Output`, because
    /// they may call functions such as [`MetricId::name`] that use the global registry.
    pub(crate) fn init_global(reg: MetricRegistry) {
        GLOBAL_METRICS.set(reg).unwrap_or_else(|_| panic!("The MetricRegistry can be initialized only once."));
    }

    /// Finds the metric that has the given id.
    pub fn with_id(&self, id: &MetricId) -> Option<&Metric> {
        self.metrics_by_id.get(id)
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
            values: self.metrics_by_id.values()
        }
    }

    /// Creates a new metric and registers it in this registry.
    pub(crate) fn create_metric(
        &mut self,
        name: &str,
        value_type: MeasurementType,
        unit: Unit,
        description: &str,
    ) -> Result<MetricId, MetricCreationError> {
        let id = MetricId(self.metrics_by_id.len());
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
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

pub struct MetricIter<'a> { values: std::collections::hash_map::Values<'a, MetricId, Metric> }
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

pub struct ElementRegistry {
    pub sources: Vec<Box<dyn Source>>,
    pub transforms: Vec<Box<dyn Transform>>,
    pub outputs: Vec<Box<dyn Output>>,
}

impl ElementRegistry {
    pub fn new() -> Self {
        ElementRegistry {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
        }
    }

    pub(crate) fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        self.sources.push(source);
    }

    pub(crate) fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        self.transforms.push(transform);
    }

    pub(crate) fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        self.outputs.push(output);
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
