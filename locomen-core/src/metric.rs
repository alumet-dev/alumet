use std::{marker::PhantomData, time::SystemTime, collections::HashMap};

/// A metric id, used for internal purposes such as storing the list of metrics.
pub(crate) struct MetricId(usize);

/// The representation of a metric.
pub struct Metric {
    id: MetricId,
    name: String,
    description: String,
    unit: Option<String>,
}

/// A resource id.
pub(crate) struct ResourceId(usize);

pub struct Resource {
    attributes: HashMap<String, AttributeValue>
}

pub struct MeasurementPoint {
    /// The metric that has been measured.
    metric: MetricId,

    /// The time of the measurement.
    timestamp: SystemTime,

    /// The measured value.
    value: MeasurementValue,

    /// What the metric is about: process, disk, etc.
    resource: ResourceId,

    /// Additional attributes on the measurement point
    attributes: Option<Box<HashMap<String, AttributeValue>>>,
    // the HashMap is Boxed to make the struct smaller, which is good for the cache

    // TODO ajouter aussi
    // une "ressource": sur quoi porte la métrique (process id, disque, hostname, etc.)
    // un "scope" ou une "source" ou "provider", càd d'où vient la métrique (nom et version du plugin par ex.)
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

pub struct MeasurementBuffer {
    /// Stores measured data points.
    points: Vec<MeasurementPoint>
}

impl MeasurementBuffer {
    fn push(&mut self, point: MeasurementPoint) {
        self.points.push(point);
    }
}
