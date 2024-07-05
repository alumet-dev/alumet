//! Foreign Function Interface for dynamically-loaded plugins.
//! 
//! To be usable by plugins in a reliable way, every exposed
//! function needs to be declared like this:
//! ```ignore
//! #[no_mangle]
//! pub extern "C" fn(...) -> ... {
//!     // ...
//! }
//! ```
//! and every exposed struct needs to be repr-C:
//! ```ignore
//! #[repr(C)]
//! pub struct ExposedStruct {
//!     // ...
//! }
//! ```

use libc::c_void;

use crate::measurement::{MeasurementAccumulator, MeasurementBuffer};
use crate::pipeline::elements::output::OutputContext;
use crate::pipeline::elements::transform::TransformContext;
use crate::plugin::AlumetStart;
use time::Timestamp;

// Submodules
pub mod config;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod resources;
pub mod units;
pub mod string;
pub mod time;

// ====== Function types ======
pub type PluginInitFn = extern "C" fn(config: *const toml::Table) -> *mut c_void;
pub type PluginDefaultConfigFn = extern "C" fn(config: *mut toml::Table);
pub type PluginStartFn = extern "C" fn(instance: *mut c_void, alumet: *mut AlumetStart);
pub type PluginStopFn = extern "C" fn(instance: *mut c_void);

pub type DropFn = unsafe extern "C" fn(instance: *mut c_void);
pub type NullableDropFn = Option<unsafe extern "C" fn(instance: *mut c_void)>;

pub type SourcePollFn = extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementAccumulator, timestamp: Timestamp);
pub type TransformApplyFn = extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementBuffer, ctx: *const FfiTransformContext);
pub type OutputWriteFn = extern "C" fn(instance: *mut c_void, buffer: *const MeasurementBuffer, ctx: *const FfiOutputContext);

// ====== OutputContext ======

#[repr(C)]
pub struct FfiOutputContext<'a> {
    inner: *const OutputContext<'a>
}

#[repr(C)]
pub struct FfiTransformContext<'a> {
    inner: *const TransformContext<'a>
}
