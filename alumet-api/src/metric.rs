use std::{collections::HashMap, time::SystemTime};

use crate::resource::ResourceId;

/// All information about a metric.
pub struct Metric {
    id: MetricId,
    name: String,
    description: Option<String>,
    unit: Option<String>,
    group: Option<MetricGroupId>,
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(PartialEq, Eq, Hash, Clone)] // not Copy because the struct may change of the future
pub struct MetricId(usize);

/// A group of metrics allows to deduplicate the attributes that are the same across all the metrics of the group.
pub struct MetricGroup {
    id: MetricGroupId,
    name: String,
    description: Option<String>,
    attributes: HashMap<String, AttributeValue>,
}

/// ID of a MetricGroup.
#[derive(PartialEq, Eq, Hash, Clone)]
pub struct MetricGroupId(usize);

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

pub struct MetricRegistry {
    metrics: HashMap<MetricId, Metric>,
    groups: HashMap<MetricGroupId, MetricGroup>,
    global_attributes: HashMap<String, AttributeValue>,
}

impl MetricRegistry {
    pub fn new() -> MetricRegistry {
        MetricRegistry {
            metrics: HashMap::new(),
            groups: HashMap::new(),
            global_attributes: HashMap::new(),
        }
    }
    
    pub fn len(&self) -> usize {
        self.metrics.len()
    }

    pub fn new_metric(&mut self, name: String) -> MetricBuilder {
        MetricBuilder::new(self, name, None)
    }

    pub fn metric(&self, id: &MetricId) -> Option<&Metric> {
        self.metrics.get(id)
    }

    pub fn metric_mut(&mut self, id: &MetricId) -> Option<&mut Metric> {
        self.metrics.get_mut(id)
    }

    pub fn group(&self, id: &MetricGroupId) -> Option<&MetricGroup> {
        self.groups.get(id)
    }

    pub fn group_mut(&mut self, id: &MetricGroupId) -> Option<&mut MetricGroup> {
        self.groups.get_mut(id)
    }

    pub fn new_group(&mut self, name: String) -> MetricGroupBuilder {
        MetricGroupBuilder::new(self, name)
    }
}

pub struct MetricBuilder<'a> {
    manager: &'a mut MetricRegistry,
    inner: Metric,
}
impl<'a> MetricBuilder<'a> {
    pub fn new(
        manager: &'a mut MetricRegistry,
        name: String,
        group: Option<MetricGroupId>,
    ) -> MetricBuilder<'a> {
        MetricBuilder {
            manager,
            inner: Metric {
                id: MetricId(0),
                name,
                description: None,
                unit: None,
                group,
            },
        }
    }

    pub fn description(&mut self, desc: String) -> &mut Self {
        self.inner.description = Some(desc);
        self
    }

    pub fn unit_custom(&mut self, unit: String) -> &mut Self {
        self.inner.unit = Some(unit);
        self
    }

    pub fn group(&mut self, group: MetricGroupId) -> &mut Self {
        self.inner.group = Some(group);
        self
    }

    pub fn build(mut self) -> MetricId {
        let id = MetricId(self.manager.metrics.len());
        self.inner.id = id.clone();
        self.manager.metrics.insert(id.clone(), self.inner);
        id
    }
}

pub struct MetricGroupBuilder<'a> {
    manager: &'a mut MetricRegistry,
    inner: MetricGroup,
}
impl<'a> MetricGroupBuilder<'a> {
    fn new(manager: &'a mut MetricRegistry, name: String) -> MetricGroupBuilder<'a> {
        MetricGroupBuilder {
            manager,
            inner: MetricGroup {
                id: MetricGroupId(0),
                name,
                description: None,
                attributes: HashMap::new(),
            },
        }
    }

    pub fn description(&mut self, desc: String) -> &mut Self {
        self.inner.description = Some(desc);
        self
    }

    pub fn attribute(&mut self, key: String, value: AttributeValue) -> &mut Self {
        self.inner.attributes.insert(key, value);
        self
    }

    pub fn build(mut self) -> MetricGroupId {
        let id = MetricGroupId(self.manager.groups.len());
        self.inner.id = id.clone();
        self.manager.groups.insert(id.clone(), self.inner);
        id
    }
}
