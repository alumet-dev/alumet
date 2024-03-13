use core::fmt;
use fxhash::FxBuildHasher;
use std::{borrow::Cow, collections::HashMap, fmt::Display, marker::PhantomData, time::SystemTime};

use crate::{pipeline::registry::MetricRegistry, units::Unit};

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
}
pub(crate) trait InternalMetricId {
    fn id(&self) -> UntypedMetricId;
}

pub(crate) fn get_metric<M: InternalMetricId>(metric: &M) -> &'static Metric {
    MetricRegistry::global().with_id(&metric.id()).unwrap_or_else(|| {
        panic!(
            "Every metric should be in the global registry, but this one was not found: {}",
            metric.id().0
        )
    })
}

/// A metric id, used for internal purposes such as storing the list of metrics.
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct UntypedMetricId(pub(crate) usize);

impl InternalMetricId for UntypedMetricId {
    fn id(&self) -> UntypedMetricId {
        self.clone()
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct TypedMetricId<T: MeasurementType>(pub(crate) usize, PhantomData<T>);

impl<T: MeasurementType> InternalMetricId for TypedMetricId<T> {
    #[inline]
    fn id(&self) -> UntypedMetricId {
        UntypedMetricId(self.0)
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

// All InternalMetricIds are MetricIds
impl<M: InternalMetricId> MetricId for M {
    fn name(&self) -> &str {
        let metric = get_metric(self);
        &metric.name
    }
}

/// A data point about a metric that has been measured.
#[derive(Clone)]
pub struct MeasurementPoint {
    /// The metric that has been measured.
    pub metric: UntypedMetricId,

    /// The time of the measurement.
    pub timestamp: SystemTime,

    /// The measured value.
    pub value: WrappedMeasurementValue,

    /// The resource this measurement is about.
    pub resource: ResourceId,

    /// Additional attributes on the measurement point.
    /// Not public because we could change how they are stored later.
    attributes: HashMap<String, AttributeValue, FxBuildHasher>,
}

impl MeasurementPoint {
    /// Creates a new MeasurementPoint without attributes.
    /// 
    /// Use [`with_attr`] or [`with_attr_vec`] to attach arbitrary attributes to the point.
    pub fn new<T: MeasurementType>(
        timestamp: SystemTime,
        metric: TypedMetricId<T>,
        resource: ResourceId,
        value: T::T,
    ) -> MeasurementPoint {
        Self::new_untyped(timestamp, UntypedMetricId(metric.0), resource, T::wrapped_value(value))
    }

    /// Creates a new MeasurementPoint without attributes, using an untyped metric.
    /// Prefer to use [`new`] with a typed metric instead.
    /// 
    /// Use [`with_attr`] or [`with_attr_vec`] to attach arbitrary attributes to the point.
    pub fn new_untyped(
        timestamp: SystemTime,
        metric: UntypedMetricId,
        resource: ResourceId,
        value: WrappedMeasurementValue,
    ) -> MeasurementPoint {
        MeasurementPoint {
            metric,
            timestamp,
            value,
            resource,
            attributes: HashMap::with_hasher(FxBuildHasher::default()),
        }
    }
    
    /// Returns the number of attributes attached to this measurement point.
    pub fn attributes_len(&self) -> usize {
        self.attributes.len()
    }

    /// Iterates on the attributes attached to the measurement point.
    pub fn attributes(&self) -> impl Iterator<Item = (&String, &AttributeValue)> {
        self.attributes.iter()
    }
    
    /// Iterates on the keys of the attributes that are attached to the point.
    pub fn attributes_keys(&self) -> impl Iterator<Item = &String> {
        self.attributes.keys()
    }
    
    pub(crate) fn add_attr(&mut self, key: &str, value: AttributeValue) {
        self.attributes.insert(key.to_owned(), value);
    }

    /// Sets an attribute on this measurement point.
    /// If an attribute with the same key already exists, its value is replaced.
    pub fn with_attr(mut self, key: &str, value: AttributeValue) -> Self {
        self.add_attr(key, value);
        self
    }

    /// Attaches multiple attributes to this measurement point, from a [`Vec`].
    /// Existing attributes with conflicting keys are replaced.
    pub fn with_attr_vec(mut self, attributes: Vec<(String, AttributeValue)>) -> Self {
        self.attributes.extend(attributes);
        self
    }

    /// Attaches multiple attributes to this measurement point, from a [`HashMap`].
    /// Existing attributes with conflicting keys are replaced.
    pub fn with_attr_map(mut self, attributes: HashMap<String, AttributeValue, FxBuildHasher>) -> Self {
        if self.attributes.is_empty() {
            self.attributes = attributes;
        } else {
            self.attributes.extend(attributes);
        }
        self
    }
}

pub trait MeasurementType {
    type T;

    fn wrapped_value(v: Self::T) -> WrappedMeasurementValue;
    fn wrapped_type() -> WrappedMeasurementType;
}
impl MeasurementType for u64 {
    type T = u64;

    fn wrapped_value(v: Self::T) -> WrappedMeasurementValue {
        WrappedMeasurementValue::U64(v)
    }

    fn wrapped_type() -> WrappedMeasurementType {
        WrappedMeasurementType::U64
    }
}
impl MeasurementType for f64 {
    type T = f64;

    fn wrapped_value(v: Self::T) -> WrappedMeasurementValue {
        WrappedMeasurementValue::F64(v)
    }

    fn wrapped_type() -> WrappedMeasurementType {
        WrappedMeasurementType::F64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub enum WrappedMeasurementType {
    F64,
    U64,
}
impl fmt::Display for WrappedMeasurementType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone)]
pub enum WrappedMeasurementValue {
    F64(f64),
    U64(u64),
}

impl WrappedMeasurementValue {
    pub fn measurement_type(&self) -> WrappedMeasurementType {
        match self {
            WrappedMeasurementValue::F64(_) => WrappedMeasurementType::F64,
            WrappedMeasurementValue::U64(_) => WrappedMeasurementType::U64,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AttributeValue {
    F64(f64),
    U64(u64),
    Bool(bool),
    String(String),
}

impl Display for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttributeValue::F64(x) => write!(f, "{x}"),
            AttributeValue::U64(x) => write!(f, "{x}"),
            AttributeValue::Bool(x) => write!(f, "{x}"),
            AttributeValue::String(str) => f.write_str(str),
        }
    }
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
        MeasurementBuffer {
            points: Vec::with_capacity(capacity),
        }
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
            ResourceId::Custom { kind, id: _ } => &kind,
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
            ResourceId::Custom { kind: _, id } => LazyDisplayable::Str(&id),
        }
    }
}

enum LazyDisplayable<'a> {
    U32(u32),
    Str(&'a str),
}

impl<'a> Display for LazyDisplayable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyDisplayable::U32(id) => write!(f, "{id}"),
            LazyDisplayable::Str(id) => write!(f, "{id}"),
        }
    }
}
