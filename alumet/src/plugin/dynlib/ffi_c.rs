use std::{
    borrow::Cow,
    ffi::{CStr, CString},
    time::{Duration, SystemTime},
};

use libc::{c_char, c_void};

use crate::{
    config,
    metrics::{
        self, AttributeValue, MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, ResourceId, UntypedMetricId,
        WrappedMeasurementType, WrappedMeasurementValue,
    },
    pipeline,
    units::Unit,
};

use super::super::AlumetStart;

pub type PluginInitFn = extern "C" fn(config: *const config::ConfigTable) -> *mut c_void;
pub type PluginStartFn = extern "C" fn(instance: *mut c_void, alumet: *mut AlumetStart);
pub type PluginStopFn = extern "C" fn(instance: *mut c_void);
pub type DropFn = unsafe extern "C" fn(instance: *mut c_void);
pub type NullableDropFn = Option<unsafe extern "C" fn(instance: *mut c_void)>;

pub type SourcePollFn = extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementAccumulator, timestamp: Timestamp);
pub type TransformApplyFn = extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementBuffer);
pub type OutputWriteFn = extern "C" fn(instance: *mut c_void, buffer: *const MeasurementBuffer);

// ====== C-compatible alternative to some Rust types ======

#[repr(C)]
pub struct Timestamp {
    secs: u64,
    nanos: u32,
}

impl From<SystemTime> for Timestamp {
    fn from(value: SystemTime) -> Self {
        let diff = value
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Every timestamp should be obtained from system_time_now()");
        Timestamp {
            secs: diff.as_secs(),
            nanos: diff.subsec_nanos(),
        }
    }
}

impl From<Timestamp> for SystemTime {
    fn from(value: Timestamp) -> Self {
        std::time::UNIX_EPOCH + Duration::new(value.secs, value.nanos)
    }
}

#[derive(Debug)]
#[repr(C)]
pub enum CResourceId {
    /// The whole local machine, for instance the whole physical server.
    LocalMachine,
    /// A process at the OS level.
    Process { pid: u32 },
    /// A control group, often abbreviated cgroup.
    ControlGroup { path: *const c_char },
    /// A physical CPU package (which is not the same as a NUMA node).
    CpuPackage { id: u32 },
    /// A CPU core.
    CpuCore { id: u32 },
    /// The RAM attached to a CPU package.
    Dram { pkg_id: u32 },
    /// A dedicated GPU.
    Gpu { bus_id: *const c_char },
    /// A custom resource
    Custom { kind: *const c_char, id: *const c_char },
}

impl TryFrom<&CResourceId> for ResourceId {
    type Error = std::str::Utf8Error;

    fn try_from(value: &CResourceId) -> Result<Self, Self::Error> {
        Ok(match value {
            CResourceId::LocalMachine => ResourceId::LocalMachine,
            CResourceId::Process { pid } => ResourceId::Process { pid: *pid },
            CResourceId::ControlGroup { path } => ResourceId::ControlGroup {
                path: c_string_to_cow(*path)?,
            },
            CResourceId::CpuPackage { id } => ResourceId::CpuPackage { id: *id },
            CResourceId::CpuCore { id } => ResourceId::CpuCore { id: *id },
            CResourceId::Dram { pkg_id } => ResourceId::Dram { pkg_id: *pkg_id },
            CResourceId::Gpu { bus_id } => ResourceId::Gpu {
                bus_id: c_string_to_cow(*bus_id)?,
            },
            CResourceId::Custom { kind, id } => ResourceId::Custom {
                kind: c_string_to_cow(*kind)?,
                id: c_string_to_cow(*id)?,
            },
        })
    }
}

fn c_string_to_cow(c: *const c_char) -> Result<Cow<'static, str>, std::str::Utf8Error> {
    let string = unsafe { CStr::from_ptr(c).to_str() }?.to_owned();
    Ok(Cow::Owned(string))
}

fn cow_to_c_string(c: &Cow<'static, str>) -> Result<CString, std::ffi::NulError> {
    let string = CString::new(c.as_bytes())?;
    Ok(string)
}

// ====== AlumetStart ffi ======

#[no_mangle]
pub extern "C" fn alumet_create_metric(
    alumet: &mut AlumetStart,
    name: *const c_char,
    value_type: WrappedMeasurementType,
    unit: Unit,
    description: *const c_char,
) -> UntypedMetricId {
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
    let description = unsafe { CStr::from_ptr(description) }.to_str().unwrap();
    // todo handle errors (how to pass them to C properly?)
    let metric_id = alumet
        .create_metric_untyped(name, value_type, unit, description)
        .unwrap();
    metric_id
}

#[no_mangle]
pub extern "C" fn alumet_add_source(
    alumet: &mut AlumetStart,
    source_data: *mut c_void,
    source_poll_fn: SourcePollFn,
    source_drop_fn: NullableDropFn,
) {
    let source = Box::new(FfiSource {
        data: source_data,
        poll_fn: source_poll_fn,
        drop_fn: source_drop_fn,
    });
    alumet.add_source(source);
}
#[no_mangle]
pub extern "C" fn alumet_add_transform(
    alumet: &mut AlumetStart,
    transform_data: *mut c_void,
    transform_apply_fn: TransformApplyFn,
    transform_drop_fn: NullableDropFn,
) {
    let transform = Box::new(FfiTransform {
        data: transform_data,
        apply_fn: transform_apply_fn,
        drop_fn: transform_drop_fn,
    });
    alumet.add_transform(transform);
}
#[no_mangle]
pub extern "C" fn alumet_add_output(
    alumet: &mut AlumetStart,
    output_data: *mut c_void,
    output_write_fn: OutputWriteFn,
    output_drop_fn: NullableDropFn,
) {
    let output = Box::new(FfiOutput {
        data: output_data,
        write_fn: output_write_fn,
        drop_fn: output_drop_fn,
    });
    alumet.add_output(output);
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
    resource: CResourceId,
    value: WrappedMeasurementValue,
) -> *mut MeasurementPoint {
    let resource: ResourceId = (&resource)
        .try_into()
        .unwrap_or_else(|_| panic!("invalid resource: {resource:?}"));
    let p = MeasurementPoint::new_untyped(timestamp.into(), metric, resource, value);
    Box::into_raw(Box::new(p)) // box and turn the box into a pointer, it now needs to be dropped manually
}

#[no_mangle]
pub extern "C" fn mpoint_new_u64(
    timestamp: Timestamp,
    metric: UntypedMetricId,
    resource: CResourceId,
    value: u64,
) -> *mut MeasurementPoint {
    mpoint_new(timestamp, metric, resource, WrappedMeasurementValue::U64(value))
}

#[no_mangle]
pub extern "C" fn mpoint_new_f64(
    timestamp: Timestamp,
    metric: UntypedMetricId,
    resource: CResourceId,
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
fn mpoint_attr(point: *mut MeasurementPoint, key: *const c_char, value: AttributeValue) {
    let point = unsafe { &mut *point }; // not Box::from_raw because we don't want to take ownership of the point
    let key = unsafe { CStr::from_ptr(key) }.to_str().unwrap();
    point.add_attr(key, value);
}

#[no_mangle]
pub extern "C" fn mpoint_attr_u64(point: *mut MeasurementPoint, key: *const c_char, value: u64) {
    mpoint_attr(point, key, AttributeValue::U64(value))
}

#[no_mangle]
pub extern "C" fn mpoint_attr_f64(point: *mut MeasurementPoint, key: *const c_char, value: f64) {
    mpoint_attr(point, key, AttributeValue::F64(value))
}

#[no_mangle]
pub extern "C" fn mpoint_attr_bool(point: *mut MeasurementPoint, key: *const c_char, value: bool) {
    mpoint_attr(point, key, AttributeValue::Bool(value))
}

#[no_mangle]
pub extern "C" fn mpoint_attr_str(point: *mut MeasurementPoint, key: *const c_char, value: *const c_char) {
    let value = unsafe { CStr::from_ptr(value) }.to_str().unwrap().to_string();
    mpoint_attr(point, key, AttributeValue::String(value))
}

// C version of MeasurementPoint for fast field access.
#[repr(C)]
pub struct CMeasurementPoint<'a> {
    pub metric: UntypedMetricId,
    pub timestamp: Timestamp,
    pub value: CMeasurementValue,
    _original: &'a MeasurementPoint,
}
#[repr(C)]
pub enum CMeasurementValue {
    U64(u64),
    F64(f64),
}
impl From<&WrappedMeasurementValue> for CMeasurementValue {
    fn from(value: &WrappedMeasurementValue) -> Self {
        match value {
            WrappedMeasurementValue::F64(x) => CMeasurementValue::F64(*x),
            WrappedMeasurementValue::U64(x) => CMeasurementValue::U64(*x),
        }
    }
}

impl<'a> TryFrom<&'a MeasurementPoint> for CMeasurementPoint<'a> {
    type Error = std::ffi::NulError;

    fn try_from(p: &'a MeasurementPoint) -> Result<Self, Self::Error> {
        Ok(CMeasurementPoint {
            metric: p.metric,
            timestamp: p.timestamp.into(),
            value: (&p.value).into(),
            _original: p,
        })
    }
}
// pub extern "C" fn mpoint_foreach_attribute(point: *const MeasurementPoint) -> UntypedMetricId {
//     todo!("not implemented yet");
// }

// ====== MeasurementBuffer ffi ======
#[no_mangle]
pub extern "C" fn mbuffer_len(buf: &MeasurementBuffer) -> usize {
    buf.len()
}
#[no_mangle]
pub extern "C" fn mbuffer_reserve(buf: &mut MeasurementBuffer, additional: usize) {
    buf.reserve(additional);
}

pub type ForeachPointFn = unsafe extern "C" fn(*mut c_void, CMeasurementPoint);
#[no_mangle]
pub extern "C" fn mbuffer_foreach(buf: &MeasurementBuffer, data: *mut c_void, f: ForeachPointFn) {
    for point in buf.iter() {
        // build a struct that's easier to use from C
        let c_point: CMeasurementPoint = point.try_into().expect("The content of a MeasurementPoint content should be convertible to C-compatible values.");
        unsafe { f(data, c_point) }
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

// ====== internals ======

struct FfiSource {
    data: *mut c_void,
    poll_fn: SourcePollFn,
    drop_fn: Option<DropFn>,
}
struct FfiTransform {
    data: *mut c_void,
    apply_fn: TransformApplyFn,
    drop_fn: Option<DropFn>,
}
struct FfiOutput {
    data: *mut c_void,
    write_fn: OutputWriteFn,
    drop_fn: Option<DropFn>,
}
// To be safely `Send`, sources/transforms/outputs may not use any thread-local storage,
// and the `data` pointer must not be shared with other threads.
// When implementing a non-Rust plugin, this has to be checked manually.
unsafe impl Send for FfiSource {}
unsafe impl Send for FfiTransform {}
unsafe impl Send for FfiOutput {}

impl pipeline::Source for FfiSource {
    fn poll(
        &mut self,
        into: &mut metrics::MeasurementAccumulator,
        time: SystemTime,
    ) -> Result<(), pipeline::PollError> {
        (self.poll_fn)(self.data, into, time.into());
        Ok(())
    }
}
impl pipeline::Transform for FfiTransform {
    fn apply(&mut self, on: &mut metrics::MeasurementBuffer) -> Result<(), pipeline::TransformError> {
        (self.apply_fn)(self.data, on);
        Ok(())
    }
}
impl pipeline::Output for FfiOutput {
    fn write(&mut self, measurements: &metrics::MeasurementBuffer) -> Result<(), pipeline::WriteError> {
        (self.write_fn)(self.data, measurements);
        Ok(())
    }
}

impl Drop for FfiSource {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
impl Drop for FfiTransform {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
impl Drop for FfiOutput {
    fn drop(&mut self) {
        if let Some(drop) = self.drop_fn {
            unsafe { drop(self.data) };
        }
    }
}
