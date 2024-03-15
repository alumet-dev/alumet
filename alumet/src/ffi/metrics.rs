use std::{mem::ManuallyDrop, time::SystemTime};

use libc::c_void;

use crate::{metrics::{
    AttributeValue, MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, MetricId, UntypedMetricId, WrappedMeasurementValue
}, resources::ResourceId};

use super::{resources::FfiResourceId, string::{AStr, AString}, Timestamp};

// ====== Metrics ffi ======
#[no_mangle]
pub extern "C" fn metric_name<'a>(metric: UntypedMetricId) -> AStr<'a> {
    let name: &'static str = &crate::metrics::get_metric(&metric).name;
    AStr::from(name)
}


// ====== MeasurementPoint ffi ======

#[no_mangle]
pub extern "C" fn system_time_now() -> *mut Timestamp {
    let t = Timestamp::from(SystemTime::now());
    Box::into_raw(Box::new(t))
}

/// Internal: C binding to [`MeasurementPoint::new`].
fn mpoint_new(
    timestamp: Timestamp,
    metric: UntypedMetricId,
    resource: FfiResourceId,
    value: WrappedMeasurementValue,
) -> *mut MeasurementPoint {
    let resource = ResourceId::from(resource);
    let p = MeasurementPoint::new_untyped(timestamp.into(), metric, resource, value);
    Box::into_raw(Box::new(p)) // box and turn the box into a pointer, it now needs to be dropped manually
}

#[no_mangle]
pub extern "C" fn mpoint_new_u64(
    timestamp: Timestamp,
    metric: UntypedMetricId,
    resource: FfiResourceId,
    value: u64,
) -> *mut MeasurementPoint {
    mpoint_new(timestamp, metric, resource, WrappedMeasurementValue::U64(value))
}

#[no_mangle]
pub extern "C" fn mpoint_new_f64(
    timestamp: Timestamp,
    metric: UntypedMetricId,
    resource: FfiResourceId,
    value: f64,
) -> *mut MeasurementPoint {
    mpoint_new(timestamp, metric, resource, WrappedMeasurementValue::F64(value))
}

/// Free a MeasurementPoint.
/// Do **not** call this function after pushing a point with [`mbuffer_push`] or [`maccumulator_push`].
#[no_mangle]
pub extern "C" fn mpoint_free(point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) }; // get the box back
    drop(boxed); // free memory (and call destructor)
}

/// Internal: C binding to [`MeasurementPoint::add_attr`].
fn mpoint_attr(point: *mut MeasurementPoint, key: AStr, value: AttributeValue) {
    let point = unsafe { &mut *point }; // not Box::from_raw because we don't want to take ownership of the point
    let key: &str = (&key).into();
    point.add_attr(key, value);
}

/// Generates a C-compatible function around mpoint_attr, for a specific type of attribute.
macro_rules! attr_adder {
    ( $(( $name:tt, $name2:tt, $value_type:ty, $constructor:expr )),* ) => {
        $(
            #[no_mangle]
            pub extern "C" fn $name(point: *mut MeasurementPoint, key: AStr, value: $value_type) {
                mpoint_attr(point, key, $constructor(value.into()))
            }
        )*
        
        // Also accept AString as a key
        $(
            #[no_mangle]
            pub extern "C" fn $name2(point: *mut MeasurementPoint, key: AString, value: $value_type) {
                let key = ManuallyDrop::new(key);
                mpoint_attr(point, (&*key).into(), $constructor(value.into()))
            }
        )*
    };
}

attr_adder![
    (mpoint_attr_u64, mpoint_attr_u64_s, u64, AttributeValue::U64),
    (mpoint_attr_f64, mpoint_attr_f64_s, f64, AttributeValue::F64),
    (mpoint_attr_bool, mpoint_attr_bool_s, bool, AttributeValue::Bool),
    (mpoint_attr_str, mpoint_attr_str_s, AStr, AttributeValue::String)
];

// getters

#[no_mangle]
pub extern "C" fn mpoint_metric(point: &MeasurementPoint) -> UntypedMetricId {
    point.metric
}

#[no_mangle]
pub extern "C" fn mpoint_value(point: &MeasurementPoint) -> FfiMeasurementValue {
    (&point.value).into()
}

#[no_mangle]
pub extern "C" fn mpoint_timestamp(point: &MeasurementPoint) -> Timestamp {
    point.timestamp.into()
}

#[no_mangle]
pub extern "C" fn mpoint_resource(point: &MeasurementPoint) -> FfiResourceId {
    FfiResourceId::from(point.resource.to_owned())
}

#[no_mangle]
pub extern "C" fn mpoint_resource_kind(point: &MeasurementPoint) -> AString {
    point.resource.kind().into()
}

#[no_mangle]
pub extern "C" fn mpoint_resource_id(point: &MeasurementPoint) -> AString {
    point.resource.id_str().to_string().into()
}

#[repr(C)]
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
#[no_mangle]
pub extern "C" fn mbuffer_len(buf: &MeasurementBuffer) -> usize {
    buf.len()
}
#[no_mangle]
pub extern "C" fn mbuffer_reserve(buf: &mut MeasurementBuffer, additional: usize) {
    buf.reserve(additional);
}

pub type ForeachPointFn = unsafe extern "C" fn(*mut c_void, *const MeasurementPoint);

/// Iterates on a [`MeasurementBuffer`] by calling `f(data, point)` for each point of the buffer.
#[no_mangle]
pub extern "C" fn mbuffer_foreach(buf: &MeasurementBuffer, data: *mut c_void, f: ForeachPointFn) {
    for point in buf.iter() {
        unsafe { f(data, point) };
    }
}

/// Adds a measurement to the buffer.
/// The point is consumed in the operation, you must **not** use it afterwards.
#[no_mangle]
pub extern "C" fn mbuffer_push(buf: &mut MeasurementBuffer, point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) };
    buf.push(*boxed);
}
/// Adds a measurement to the accumulator.
/// The point is consumed in the operation, you must **not** use it afterwards.
#[no_mangle]
pub extern "C" fn maccumulator_push(buf: &mut MeasurementAccumulator, point: *mut MeasurementPoint) {
    let boxed = unsafe { Box::from_raw(point) };
    buf.push(*boxed);
}
