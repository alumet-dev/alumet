use std::{collections::HashMap, time::SystemTime};

use crate::{resource::ResourceId, units::Unit};

/// All information about a metric.
pub struct Metric {
    id: MetricId,
    name: String,
    description: Option<String>,
    unit: Unit,
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(PartialEq, Eq, Hash, Clone)] // not Copy because the struct may change of the future
pub struct MetricId(usize);

/// A data point about a metric that has been measured.
pub struct MeasurementPoint {
    /// The metric that has been measured.
    metric: MetricId,

    /// The time of the measurement.
    timestamp: SystemTime,

    /// The measured value.
    value: MeasurementValue,

    /// The resource this measurement is about.
    resource: ResourceId,

    /// Additional attributes on the measurement point
    attributes: Option<Box<HashMap<String, AttributeValue>>>,
    // the HashMap is Boxed to make the struct smaller, which is good for the cache
}

impl MeasurementPoint {
    pub fn new(timestamp: SystemTime, metric: MetricId, resource: ResourceId, value: MeasurementValue) -> MeasurementPoint {
        MeasurementPoint {
            metric,
            timestamp,
            value,
            resource,
            attributes: None,
        }
    }
    
    pub fn with_attrs(metric: MetricId, resource: ResourceId, value: MeasurementValue, attrs: Box<HashMap<String, AttributeValue>>) -> MeasurementPoint {
        MeasurementPoint {
            metric,
            timestamp: SystemTime::now(),
            value,
            resource,
            attributes: Some(attrs),
        }
    }
}

pub enum MeasurementValue {
    Float(f64),
    UInt(u64),
}

pub enum AttributeValue {
    Float(f64),
    UInt(u64),
    Bool(bool),
    String(String),
}

/// A `MeasurementBuffer` stores measured data points.
pub struct MeasurementBuffer {
    points: Vec<MeasurementPoint>,
}

impl MeasurementBuffer {
    pub fn new() -> MeasurementBuffer {
        MeasurementBuffer { points: Vec::new() }
    }

    pub fn push(&mut self, point: MeasurementPoint) {
        self.points.push(point);
    }

    pub fn iter(&self) -> impl Iterator<Item = &MeasurementPoint> {
        self.points.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut MeasurementPoint> {
        self.points.iter_mut()
    }
}

#[derive(Debug)]
pub enum RegistryError {
    Conflict,
    AlreadyRegistered,
}

pub struct MetricRegistry {
    metrics_by_id: HashMap<MetricId, Metric>,
    metrics_by_name: HashMap<String, MetricId>,
}

impl MetricRegistry {
    pub fn new() -> MetricRegistry {
        MetricRegistry {
            metrics_by_id: HashMap::new(),
            metrics_by_name: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.metrics_by_id.len()
    }

    /// Creates a new metric.
    ///
    /// The metric is registered when `build()` is called, and its `MetricId` is returned.
    ///
    /// # Example
    /// ```
    /// # let metrics = MetricRegistry::new();
    /// let my_metric = metrics.new_builder("light-consumption")
    ///     .unit(Units::Joules)
    ///     .description("electricity consumption of the connected light bulb")
    ///     .build();
    /// ```
    pub fn new_builder(&mut self, name: &str) -> MetricBuilder {
        MetricBuilder::new(self, name.to_owned())
    }

    /// Registers a new metric.
    fn register(&mut self, mut m: Metric) -> Result<MetricId, RegistryError> {
        let id = MetricId(self.len());
        m.id = id.clone();
        if let Some(name_conflict) = self.metrics_by_name.get(&m.name) {
            return Err(RegistryError::Conflict)
        }
        let id_conflict = self.metrics_by_id.insert(id.clone(), m);
        debug_assert!(id_conflict.is_none(), "metrics ids must be unique");
        Ok(id)
    }

    pub fn get(&self, id: &MetricId) -> Option<&Metric> {
        self.metrics_by_id.get(id)
    }

    pub fn get_by_name(&self, metric_name: &str) -> Option<&Metric> {
        self.metrics_by_name
            .get(metric_name)
            .and_then(|id| self.metrics_by_id.get(id))
    }
}

pub struct MetricBuilder<'a> {
    registry: &'a mut MetricRegistry,
    inner: Metric,
}

impl<'a> MetricBuilder<'a> {
    pub fn new(registry: &'a mut MetricRegistry, name: String) -> MetricBuilder<'a> {
        MetricBuilder {
            registry,
            inner: Metric {
                id: MetricId(0),
                name,
                description: None,
                unit: Unit::Unity,
            },
        }
    }

    pub fn description(mut self, desc: &str) -> Self {
        self.inner.description = Some(desc.to_owned());
        self
    }

    pub fn unit(mut self, unit: Unit) -> Self {
        self.inner.unit = unit;
        self
    }

    pub fn build(self) -> Result<MetricId, RegistryError> {
        self.registry.register(self.inner)
    }
}
