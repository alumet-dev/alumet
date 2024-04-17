use std::ffi::{c_char, CStr};
use std::time::Instant;

use libc::c_void;

use crate::measurement::WrappedMeasurementType;
use crate::metrics::RawMetricId;
use crate::{plugin::AlumetStart, units::Unit};

use super::pipeline::{FfiOutput, FfiTransform};
use super::time::TimeDuration;
use super::units::FfiUnit;
use super::{pipeline::FfiSource, string::AStr, NullableDropFn, SourcePollFn};
use super::{OutputWriteFn, TransformApplyFn};

#[no_mangle]
pub extern "C" fn alumet_create_metric(
    alumet: &mut AlumetStart,
    name: AStr,
    value_type: WrappedMeasurementType,
    unit: FfiUnit,
    description: AStr,
) -> RawMetricId {
    // todo handle errors (how to pass them to FFI properly?)
    let name = (&name).into();
    let description = (&description).into();
    let unit = Unit::from(unit);
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
    unit: FfiUnit,
    description: *const c_char,
) -> RawMetricId {
    // todo handle errors (how to pass them to C properly?)
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap();
    let description = unsafe { CStr::from_ptr(description) }.to_str().unwrap();
    let unit = Unit::from(unit);
    let metric_id = alumet
        .create_metric_untyped(name, value_type, unit, description)
        .unwrap();
    metric_id
}

#[no_mangle]
pub extern "C" fn alumet_add_source(
    alumet: &mut AlumetStart,
    source_data: *mut c_void,
    poll_interval: TimeDuration,
    flush_interval: TimeDuration,
    source_poll_fn: SourcePollFn,
    source_drop_fn: NullableDropFn,
) {
    let source = Box::new(FfiSource {
        data: source_data,
        poll_fn: source_poll_fn,
        drop_fn: source_drop_fn,
    });
    alumet.add_source(
        source,
        crate::pipeline::trigger::Trigger::TimeInterval {
            start_time: Instant::now(),
            poll_interval: poll_interval.into(),
            flush_interval: flush_interval.into(),
        },
    );
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
