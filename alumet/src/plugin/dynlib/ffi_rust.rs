//! FFI declarations for Rust plugins.
//! Unlike `dyn_ffi`, `dyn_ffi_rust` uses data structures that closely match the Rust standard library,
//! in order to avoid the conversion overhead. In particular, all strings are length-prefixed here, not null-terminated.
use crate::metrics::{AttributeValue, MeasurementPoint};

#[repr(C)]
pub struct FfiString {
    length: usize,
    buf: *mut u8,
}

// #[no_mangle]
// pub extern fn rust_mpoint_attr(point: *mut MeasurementPoint, key: &str, value: AttributeValue) {
//     let mut point = unsafe { Box::from_raw(point) };
//     point.add_attr(key, value); // TODO
// }
