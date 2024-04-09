//! Measurement points and buffers.
//! 
//! Each step of the Alumet pipeline reads, produces or modifies timeseries data points,
//! each represented as a [`MeasurementPoint`].
//! This is usually done through a [`MeasurementBuffer`] (for transforms and outputs)
//! or a [`MeasurementAccumulator`] (for sources).
//! 
//! ## Producing measurements
//! 
//! Assuming that you have a `buffer: &mut MeasurementBuffer` (or `MeasurementAccumulator`),
//! you can produce new measurements like this:
//! ```no_run
//! use alumet::measurement::{MeasurementBuffer, MeasurementPoint};
//! 
//! # let buffer = MeasurementBuffer::new();
//! buffer.push(MeasurementPoint::new(
//!     timestamp, // timestamp, provided by Alumet as a parameter of [Source::poll]
//!     my_metric, // a TypedMetricId that you obtained from [AlumetStart::create_metric]
//!     ResourceId::CpuPackage { id: 0 }, // the object that you are measuring
//!     value: 1234, // the measurement value
//! ));
//! ```

use core::fmt;
use fxhash::FxBuildHasher;
use std::{collections::HashMap, fmt::Display, time::SystemTime};

use super::metrics::{RawMetricId, TypedMetricId};
use super::resources::ResourceId;

/// A value that has been measured at a given point in time.
/// 
/// Measurement points may also have attributes.
/// Only certain types of values and attributes are allowed, see [`MeasurementType`] and [`AttributeValue`].
#[derive(Clone)]
pub struct MeasurementPoint {
    /// The metric that has been measured.
    pub metric: RawMetricId,

    /// The time of the measurement.
    pub timestamp: SystemTime,

    /// The measured value.
    pub value: WrappedMeasurementValue,

    /// The resource this measurement is about: CPU socket, GPU, process, ...
    pub resource: ResourceId,

    /// Additional attributes on the measurement point.
    /// Not public because we could change how they are stored later.
    attributes: HashMap<String, AttributeValue, FxBuildHasher>,
}

impl MeasurementPoint {
    /// Creates a new `MeasurementPoint` without attributes.
    ///
    /// Use [`with_attr`](Self::with_attr) or [`with_attr_vec`](Self::with_attr_vec)
    /// to attach arbitrary attributes to the point.
    pub fn new<T: MeasurementType>(
        timestamp: SystemTime,
        metric: TypedMetricId<T>,
        resource: ResourceId,
        value: T::T,
    ) -> MeasurementPoint {
        Self::new_untyped(timestamp, metric.0, resource, T::wrapped_value(value))
    }

    /// Creates a new `MeasurementPoint` without attributes, using an untyped metric.
    /// Prefer to use [`MeasurementPoint::new`] with a typed metric instead.
    ///
    /// Use [`with_attr`](Self::with_attr) or [`with_attr_vec`](Self::with_attr_vec)
    /// to attach arbitrary attributes to the point.
    pub fn new_untyped(
        timestamp: SystemTime,
        metric: RawMetricId,
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

/// Trait implemented by types that are accepted as measurement values.
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

/// Enum of the possible measurement types.
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

/// A measurement value of any supported measurement type.
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

/// An attribute value of any supported attribute type.
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
/// Unlike a [`MeasurementBuffer`], the accumulator only allows to [`push`](MeasurementAccumulator::push) new points, not to modify them.
pub struct MeasurementAccumulator<'a>(&'a mut MeasurementBuffer);

impl<'a> MeasurementAccumulator<'a> {
    /// Adds a new measurement to this accumulator.
    /// The measurement points are not deduplicated by the accumulator.
    pub fn push(&mut self, point: MeasurementPoint) {
        self.0.push(point)
    }
}
