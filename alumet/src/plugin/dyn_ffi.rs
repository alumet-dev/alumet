use std::{ffi::CStr, time::SystemTime};

use libc::{c_void, c_char};

use crate::{config, metrics::{self, MeasurementBuffer, MeasurementPoint, MeasurementType}, pipeline, units::Unit};

use super::AlumetStart;

pub type InitFn = extern "C" fn(config: *const config::ConfigTable) -> *mut c_void;
pub type StartFn = extern "C" fn(instance: *mut c_void, alumet: *mut AlumetStart);
pub type StopFn = extern "C" fn(instance: *mut c_void);
pub type DropFn = extern "C" fn(instance: *mut c_void);

pub struct TimeSpec(SystemTime);
pub type SourcePollFn = extern "C" fn(instance: *mut c_void, buffer: *mut metrics::MeasurementAccumulator, timestamp: &TimeSpec);
pub type TransformApplyFn = extern "C" fn(instance: *mut c_void, buffer: *mut metrics::MeasurementBuffer);
pub type OutputWriteFn = extern "C" fn(instance: *mut c_void, buffer: *const metrics::MeasurementBuffer);

// ====== AlumetStart ffi ======

#[no_mangle]
pub extern fn alumet_create_metric(alumet: &mut AlumetStart,
    name: *const c_char,
    value_type: MeasurementType,
    unit: Unit,
    description: *const c_char
) -> u64 {
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
    let description = unsafe { CStr::from_ptr(description)}.to_str().unwrap();
    // todo handle errors (how to pass them to C properly?)
    let metric_id = alumet.create_metric(name, value_type, unit, description).unwrap();
    metric_id.0 as u64
}

#[no_mangle]
pub extern fn alumet_add_source(alumet: &mut AlumetStart, source_data: *mut c_void, source_poll_fn: SourcePollFn) {
    let source = Box::new(FfiSource {
        data: source_data,
        poll_fn: source_poll_fn,
    });
    alumet.add_source(source);
}
#[no_mangle]
pub extern fn alumet_add_transform(alumet: &mut AlumetStart, transform_data: *mut c_void, transform_apply_fn: TransformApplyFn) {
    let transform = Box::new(FfiTransform {
        data: transform_data,
        apply_fn: transform_apply_fn,
    });
    alumet.add_transform(transform);
}
#[no_mangle]
pub extern fn alumet_add_output(alumet: &mut AlumetStart, output_data: *mut c_void, output_write_fn: OutputWriteFn) {
    let output = Box::new(FfiOutput {
        data: output_data,
        write_fn: output_write_fn,
    });
    alumet.add_output(output);
}

// ====== MeasurementPoint ffi ======
// TODO

// ====== MeasurementBuffer ffi ======
#[no_mangle]
pub extern fn mbuffer_len(buf: &MeasurementBuffer) -> usize {
    buf.len()
}
#[no_mangle]
pub extern fn mbuffer_reserve(buf: &mut MeasurementBuffer, additional: usize) {
    buf.reserve(additional);
}
#[no_mangle]
pub extern fn mbuffer_push(buf: &mut MeasurementBuffer, point: MeasurementPoint) {
    buf.push(point);
}
#[no_mangle]
pub extern fn maccumulator_push(buf: &mut MeasurementBuffer, point: MeasurementPoint) {
    buf.push(point);
}

// ====== internals ======

struct FfiSource {
    data: *mut c_void,
    poll_fn: SourcePollFn,
}
struct FfiTransform {
    data: *mut c_void,
    apply_fn: TransformApplyFn,
}
struct FfiOutput {
    data: *mut c_void,
    write_fn: OutputWriteFn,
}
// To be safely `Send`, sources/transforms/outputs may not use any thread-local storage,
// and the `data` pointer must not be shared with other threads.
// When implementing a non-Rust plugin, this has to be checked manually.
unsafe impl Send for FfiSource {}
unsafe impl Send for FfiTransform {}
unsafe impl Send for FfiOutput {}

impl pipeline::Source for FfiSource {
    fn poll(&mut self, into: &mut metrics::MeasurementAccumulator, time: SystemTime) -> Result<(), pipeline::PollError> {
        (self.poll_fn)(self.data, into, &TimeSpec(time));
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
