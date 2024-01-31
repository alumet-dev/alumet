use std::{collections::HashMap, time::SystemTime};

use crate::units::Unit;

/// All information about a metric.
pub struct Metric {
    pub id: MetricId,
    pub name: String,
    pub description: String,
    pub value_type: MetricType,
    pub unit: Unit,
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct MetricId(pub usize);

impl MetricId {
    pub fn name(&self) -> &str {
        todo!("call the registry to get the name")
    }
}

/// A data point about a metric that has been measured.
#[derive(Clone)]
pub struct MeasurementPoint {
    /// The metric that has been measured.
    pub metric: MetricId,

    /// The time of the measurement.
    pub timestamp: SystemTime,

    /// The measured value.
    pub value: MeasurementValue,

    /// The resource this measurement is about.
    pub resource: ResourceId,

    /// Additional attributes on the measurement point
    attributes: Box<HashMap<String, AttributeValue>>,
    // the HashMap is Boxed to make the struct smaller, which is good for the cache
}

impl MeasurementPoint {
    pub fn new(
        timestamp: SystemTime,
        metric: MetricId,
        resource: ResourceId,
        value: MeasurementValue,
    ) -> MeasurementPoint {
        MeasurementPoint {
            metric,
            timestamp,
            value,
            resource,
            attributes: Box::new(HashMap::new()),
        }
    }
}

#[derive(Clone)]
pub enum MetricType {
    Float,
    UInt,
}

#[derive(Debug, Clone)]
pub enum MeasurementValue {
    Float(f64),
    UInt(u64),
}

#[derive(Clone)]
pub enum AttributeValue {
    Float(f64),
    UInt(u64),
    Bool(bool),
    String(String),
}

/// A `MeasurementBuffer` stores measured data points.
/// Unlike a [`MeasurementAccumulator`], the buffer allows to modify the measurements.
#[derive(Clone)]
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

    pub fn as_accumulator(&mut self) -> MeasurementAccumulator {
        MeasurementAccumulator(self)
    }
}

impl std::fmt::Debug for MeasurementBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeasurementBuffer").field("len", &self.points.len()).finish()
    }
}


/// An accumulator stores measured data points.
/// Unlike a [`MeasurementBuffer`], the accumulator only allows to [`push()`] new points, not to modify them.
pub struct MeasurementAccumulator<'a>(&'a mut MeasurementBuffer);

impl<'a> MeasurementAccumulator<'a> {
    /// Adds a new measurement to this accumulator.
    /// The measurement points are not deduplicated by the accumulator.
    pub fn push(&mut self, point: MeasurementPoint) {
        self.0.push(point)
    }
}
/// Hardware or software entity for which metrics can be gathered.
#[non_exhaustive]
#[repr(u8)]
#[derive(Clone)]
pub enum ResourceId {
    /// The whole local machine, for instance the whole physical server.
    LocalMachine,
    /// A process at the OS level.
    Process { pid: u32 },
    /// A control group, often abbreviated cgroup.
    ControlGroup { path: String },
    /// A physical CPU package (which is not the same as a NUMA node).
    CpuPackage { id: u32 },
    /// A CPU core.
    CpuCore { id: u32 },
    /// The RAM attached to a CPU package.
    Dram { pkg_id: u32 },
    /// A dedicated GPU.
    Gpu { bus_id: String },
    /// A custom resource
    Custom { kind: String, id: String },
}

impl ResourceId {
    pub fn custom(kind: &str, id: &str) -> ResourceId {
        ResourceId::Custom { kind: kind.to_owned(), id: id.to_owned() }
    }
}
