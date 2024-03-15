use std::ffi::{c_char, CStr};

use libc::c_void;

use crate::{plugin::AlumetStart, units::Unit};
use crate::metrics::{UntypedMetricId, WrappedMeasurementType};

use super::pipeline::{FfiOutput, FfiTransform};
use super::{OutputWriteFn, TransformApplyFn};
use super::{string::AStr, NullableDropFn, SourcePollFn, pipeline::FfiSource};

#[no_mangle]
pub extern "C" fn alumet_create_metric(
    alumet: &mut AlumetStart,
    name: AStr,
    value_type: WrappedMeasurementType,
    unit: Unit,
    description: AStr,
) -> UntypedMetricId {
    // todo handle errors (how to pass them to FFI properly?)
    let name = (&name).into();
    let description = (&description).into();
    let metric_id = alumet
        .create_metric_untyped(name, value_type, unit, description)
        .unwrap();
    metric_id
}

#[no_mangle]
pub extern "C" fn alumet_create_metric_c(
    alumet: &mut AlumetStart,
    name: *const c_char,
    value_type: WrappedMeasurementType,
    unit: Unit,
    description: *const c_char,
) -> UntypedMetricId {
    // todo handle errors (how to pass them to C properly?)
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
    let description = unsafe { CStr::from_ptr(description) }.to_str().unwrap();
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
