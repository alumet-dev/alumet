// use std::ffi::{c_char, CString, CStr};
// // use libc::size_t;

// pub struct MetricId(u32);

// pub struct PluginMetadata {
//     name: String,
//     version: String,
// }

// #[no_mangle]
// pub extern "C" fn register_metric_int(name: *const c_char) -> MetricId {
//     let str = unsafe { CStr::from_ptr(name) };
//     todo!()
// }

pub mod metrics;
