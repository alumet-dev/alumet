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

use alumet::measurement::{MeasurementAccumulator, MeasurementBuffer};
use alumet::pipeline::elements::output::OutputContext;
use alumet::pipeline::elements::transform::TransformContext;
use alumet::plugin::AlumetPluginStart;
use libc::c_void;
use time::Timestamp;

#[cfg(feature = "dynamic")]
pub mod dynload;

pub mod config;
pub mod metrics;
pub mod pipeline;
pub mod plugin;
pub mod resources;
pub mod string;
pub mod time;
pub mod units;

// ====== Function types ======
pub type PluginInitFn = extern "C" fn(config: *const toml::Table) -> *mut c_void;
pub type PluginDefaultConfigFn = extern "C" fn(config: *mut toml::Table);
pub type PluginStartFn = extern "C" fn(instance: *mut c_void, alumet: *mut AlumetPluginStart);
pub type PluginStopFn = extern "C" fn(instance: *mut c_void);

pub type DropFn = unsafe extern "C" fn(instance: *mut c_void);
pub type NullableDropFn = Option<unsafe extern "C" fn(instance: *mut c_void)>;

pub type SourcePollFn = extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementAccumulator, timestamp: Timestamp);
pub type TransformApplyFn =
    extern "C" fn(instance: *mut c_void, buffer: *mut MeasurementBuffer, ctx: *const FfiTransformContext);
pub type OutputWriteFn =
    extern "C" fn(instance: *mut c_void, buffer: *const MeasurementBuffer, ctx: *const FfiOutputContext);

// ====== OutputContext ======

#[repr(C)]
pub struct FfiOutputContext<'a> {
    inner: *const OutputContext<'a>,
}

#[repr(C)]
pub struct FfiTransformContext<'a> {
    inner: *const TransformContext<'a>,
}

/// Workaround because cbindgen doesn't handle the use of types from external crates properly:
/// it should generate an opaque type for ConfigTable and ConfigArray, but it does not.
#[allow(dead_code)]
mod cbindgen_workaround {

    macro_rules! opaque_type {
        ($t:ident, $f:ident) => {
            pub struct $t;
            #[no_mangle]
            fn $f(_x: $t) {}
        };
    }

    opaque_type!(ConfigArray, __workaround_0);
    opaque_type!(MeasurementBuffer, __workaround_1);
    opaque_type!(MeasurementAccumulator, __workaround_2);
    opaque_type!(MeasurementPoint, __workaround_3);
    opaque_type!(Table, __workaround_4);
    opaque_type!(OutputContext, __workaround_5);
    opaque_type!(TransformContext, __workaround_6);
    opaque_type!(AlumetPluginStart, __workaround_7);
    opaque_type!(WrappedMeasurementValue, __workaround_8);

    #[repr(C)]
    pub enum WrappedMeasurementType {
        F64,
        U64,
    }

    #[repr(C)]
    pub struct RawMetricId(usize);
    #[no_mangle]
    pub fn __workaround_raw_metric_id(_id: RawMetricId) {}
}
