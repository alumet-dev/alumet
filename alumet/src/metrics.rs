use core::fmt;
use std::{borrow::Cow, collections::HashMap, fmt::Display, time::SystemTime};

use crate::{pipeline::registry::MetricRegistry, units::Unit};

/// All information about a metric.
pub struct Metric {
    pub id: MetricId,
    pub name: String,
    pub description: String,
    pub value_type: MeasurementType,
    pub unit: Unit,
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct MetricId(pub(crate) usize);

impl MetricId {
    pub fn name(&self) -> &str {
        let metric = MetricRegistry::global().with_id(self).unwrap_or_else(|| {
            panic!(
                "Every metric should be in the global registry, but this one was not found: {}",
                self.0
            )
        });
        &metric.name
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
    attributes: Option<HashMap<String, AttributeValue>>,
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
            attributes: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasurementType {
    Float,
    UInt,
}
impl fmt::Display for MeasurementType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone)]
pub enum MeasurementValue {
    Float(f64),
    UInt(u64),
}

impl MeasurementValue {
    pub fn measurement_type(&self) -> MeasurementType {
        match self {
            MeasurementValue::Float(_) => MeasurementType::Float,
            MeasurementValue::UInt(_) => MeasurementType::UInt,
        }
    }
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
    /// Constructs a new buffer.
    pub fn new() -> MeasurementBuffer {
        MeasurementBuffer { points: Vec::new() }
    }
    
    /// Constructs a new buffer with at least the specified capacity (allocated on construction).
    pub fn with_capacity(capacity: usize) -> MeasurementBuffer {
        MeasurementBuffer { points: Vec::with_capacity(capacity) }
    }
    
    /// Returns the number of measurement points in the buffer.
    pub fn len(&self) -> usize {
        self.points.len()
    }
    
    /// Reserves capacity for at least `additional` more elements.
    /// See [`Vec::reserve`].
    pub fn reserve(&mut self, additional: usize) {
        self.points.reserve(additional);
    }
    
    /// Adds a measurement to the buffer.
    /// The measurement points are *not* automatically deduplicated by the buffer.
    pub fn push(&mut self, point: MeasurementPoint) {
        self.points.push(point);
    }

    /// Creates an iterator on the buffer's content.
    pub fn iter(&self) -> impl Iterator<Item = &MeasurementPoint> {
        self.points.iter()
    }

    /// Creates an iterator that allows to modify the measurements.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut MeasurementPoint> {
        self.points.iter_mut()
    }

    /// Returns a `MeasurementAccumulator` that will push all measurements to this buffer.
    pub fn as_accumulator(&mut self) -> MeasurementAccumulator {
        MeasurementAccumulator(self)
    }
}

impl std::fmt::Debug for MeasurementBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeasurementBuffer")
            .field("len", &self.points.len())
            .finish()
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceId {
    /// The whole local machine, for instance the whole physical server.
    LocalMachine,
    /// A process at the OS level.
    Process { pid: u32 },
    /// A control group, often abbreviated cgroup.
    ControlGroup { path: StrCow },
    /// A physical CPU package (which is not the same as a NUMA node).
    CpuPackage { id: u32 },
    /// A CPU core.
    CpuCore { id: u32 },
    /// The RAM attached to a CPU package.
    Dram { pkg_id: u32 },
    /// A dedicated GPU.
    Gpu { bus_id: StrCow },
    /// A custom resource
    Custom { kind: StrCow, id: StrCow },
}

/// Alias to a static cow. It helps avoiding allocations of Strings.
pub type StrCow = Cow<'static, str>;

impl ResourceId {
    /// Creates a new [`ResourceId::Custom`] with the given kind and id.
    /// You can pass `&'static str` as kind, id, or both in order to avoid allocating memory.
    /// Strings are also accepted and will be moved into the ResourceId.
    pub fn custom(kind: impl Into<StrCow>, id: impl Into<StrCow>) -> ResourceId {
        ResourceId::Custom {
            kind: kind.into(),
            id: id.into(),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            ResourceId::LocalMachine => "local_machine",
            ResourceId::Process { .. } => "process",
            ResourceId::ControlGroup { .. } => "cgroup",
            ResourceId::CpuPackage { .. } => "cpu_package",
            ResourceId::CpuCore { .. } => "cpu_core",
            ResourceId::Dram { .. } => "dram",
            ResourceId::Gpu { .. } => "gpu",
            ResourceId::Custom { kind, id } => &kind,
        }
    }
    
    pub fn id_str(&self) -> impl Display + '_ {
        match self {
            ResourceId::LocalMachine => LazyDisplayable::Str(""),
            ResourceId::Process { pid } => LazyDisplayable::U32(*pid),
            ResourceId::ControlGroup { path } => LazyDisplayable::Str(&path),
            ResourceId::CpuPackage { id } => LazyDisplayable::U32(*id),
            ResourceId::CpuCore { id } => LazyDisplayable::U32(*id),
            ResourceId::Dram { pkg_id } => LazyDisplayable::U32(*pkg_id),
            ResourceId::Gpu { bus_id } => LazyDisplayable::Str(&bus_id),
            ResourceId::Custom { kind, id } => LazyDisplayable::Str(&id),
        }
    }
}

enum LazyDisplayable<'a> {
    U32(u32),
    U64(u64),
    Str(&'a str)
}

impl<'a> Display for LazyDisplayable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyDisplayable::U32(id) => write!(f, "{id}"),
            LazyDisplayable::U64(id) => write!(f, "{id}"),
            LazyDisplayable::Str(id) => write!(f, "{id}"),
        }
    }
}