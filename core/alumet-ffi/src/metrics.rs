use std::{borrow::Cow, time::SystemTime};

use libc::c_void;

use alumet::{
    measurement::{
        AttributeValue, MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue,
    },
    metrics::{def::RawMetricId, registry::MetricRegistry},
    resources::{Resource, ResourceConsumer},
};

use super::{
    FfiOutputContext,
    resources::{FfiConsumerId, FfiResourceId},
    string::{AStr, AString},
    time::Timestamp,
};

// ====== Metrics ffi ======
#[unsafe(no_mangle)]
pub extern "C" fn metric_name<'a>(metric: RawMetricId, ctx: &'a FfiOutputContext) -> AStr<'a> {
    let metrics: &MetricRegistry = unsafe { &*ctx.inner }.metrics;
    let name: &str = &metrics.by_id(&metric).unwrap().name;
    AStr::from(name)
}

// ====== MeasurementPoint ffi ======

#[unsafe(no_mangle)]
pub extern "C" fn system_time_now() -> *mut Timestamp {
    let t = Timestamp::from(SystemTime::now());
    Box::into_raw(Box::new(t))
}

/// Internal: C binding to [`MeasurementPoint::new`].
fn mpoint_new(
    timestamp: Timestamp,
    metric: RawMetricId,
    resource: FfiResourceId,
    consumer: FfiConsumerId,
    value: WrappedMeasurementValue,
) -> *mut MeasurementPoint {
    let resource = Resource::from(resource);
    let consumer = ResourceConsumer::from(consumer);
    let p = MeasurementPoint::new_untyped(timestamp.into(), metric, resource, consumer, value);
    Box::into_raw(Box::new(p)) // box and turn the box into a pointer, it now needs to be dropped manually
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_new_u64(
    timestamp: Timestamp,
    metric: RawMetricId,
    resource: FfiResourceId,
    consumer: FfiConsumerId,
    value: u64,
) -> *mut MeasurementPoint {
    mpoint_new(
        timestamp,
        metric,
        resource,
        consumer,
        WrappedMeasurementValue::U64(value),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_new_f64(
    timestamp: Timestamp,
    metric: RawMetricId,
    resource: FfiResourceId,
    consumer: FfiConsumerId,
    value: f64,
) -> *mut MeasurementPoint {
    mpoint_new(
        timestamp,
        metric,
        resource,
        consumer,
        WrappedMeasurementValue::F64(value),
    )
}

/// Free a MeasurementPoint.
/// Do **not** call this function after pushing a point with [`mbuffer_push`] or [`maccumulator_push`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpoint_free(point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) }; // get the box back
    drop(boxed); // free memory (and call destructor)
}

/// Internal: C binding to [`MeasurementPoint::add_attr`].
fn mpoint_attr(point: *mut MeasurementPoint, key: AStr, value: AttributeValue) {
    let point = unsafe { &mut *point }; // not Box::from_raw because we don't want to take ownership of the point
    let key = Cow::Owned(key.to_string());
    point.add_attr(key, value);
}

/// Generates a C-compatible function around mpoint_attr, for a specific type of attribute.
macro_rules! attr_adder {
    ( $name:tt, $value_type:ty, $constructor:expr ) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(point: *mut MeasurementPoint, key: AStr, value: $value_type) {
            mpoint_attr(point, key, $constructor(value.into()))
        }
    };
}

attr_adder!(mpoint_attr_u64, u64, AttributeValue::U64);
attr_adder!(mpoint_attr_f64, f64, AttributeValue::F64);
attr_adder!(mpoint_attr_bool, bool, AttributeValue::Bool);
attr_adder!(mpoint_attr_str, AStr, AttributeValue::String);

// getters

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_metric(point: &MeasurementPoint) -> RawMetricId {
    point.metric
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_value(point: &MeasurementPoint) -> FfiMeasurementValue {
    (&point.value).into()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_timestamp(point: &MeasurementPoint) -> Timestamp {
    point.timestamp.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_resource(point: &MeasurementPoint) -> FfiResourceId {
    FfiResourceId::from(point.resource.to_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_resource_kind(point: &MeasurementPoint) -> AString {
    point.resource.kind().into()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_resource_id(point: &MeasurementPoint) -> AString {
    point.resource.id_display().to_string().into()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_consumer(point: &MeasurementPoint) -> FfiConsumerId {
    FfiConsumerId::from(point.consumer.to_owned())
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_consumer_kind(point: &MeasurementPoint) -> AString {
    point.consumer.kind().into()
}

#[unsafe(no_mangle)]
pub extern "C" fn mpoint_consumer_id(point: &MeasurementPoint) -> AString {
    point.consumer.id_display().to_string().into()
}

#[repr(C)]
#[allow(unused)]
pub enum FfiMeasurementValue {
    U64(u64),
    F64(f64),
}
impl From<&WrappedMeasurementValue> for FfiMeasurementValue {
    fn from(value: &WrappedMeasurementValue) -> Self {
        match value {
            WrappedMeasurementValue::F64(x) => FfiMeasurementValue::F64(*x),
            WrappedMeasurementValue::U64(x) => FfiMeasurementValue::U64(*x),
        }
    }
}

// ====== MeasurementBuffer ffi ======
#[unsafe(no_mangle)]
pub extern "C" fn mbuffer_len(buf: &MeasurementBuffer) -> usize {
    buf.len()
}
#[unsafe(no_mangle)]
pub extern "C" fn mbuffer_reserve(buf: &mut MeasurementBuffer, additional: usize) {
    buf.reserve(additional);
}

pub type ForeachPointFn = unsafe extern "C" fn(*mut c_void, *const MeasurementPoint);

/// Iterates on a [`MeasurementBuffer`] by calling `f(data, point)` for each point of the buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbuffer_foreach(buf: &MeasurementBuffer, data: *mut c_void, f: ForeachPointFn) {
    for point in buf.iter() {
        unsafe { f(data, point) };
    }
}

/// Adds a measurement to the buffer.
/// The point is consumed in the operation, you must **not** use it afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbuffer_push(buf: &mut MeasurementBuffer, point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) };
    buf.push(*boxed);
}
/// Adds a measurement to the accumulator.
/// The point is consumed in the operation, you must **not** use it afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn maccumulator_push(buf: &mut MeasurementAccumulator, point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) };
    buf.push(*boxed);
}
