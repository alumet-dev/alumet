//! Support for performance events encoded via libpfm4.
//!
//! The generic `hardware`/`software`/`cache` events exposed by the kernel are a small,
//! vendor-neutral subset. CPUs also expose hundreds of PMU events whose raw
//! encodings differ per microarchitecture. [libpfm4](https://perfmon2.sourceforge.net/)
//! holds those encoding tables: given a human-readable name (e.g. `RESOURCE_STALLS:ANY`)
//! it fills a [`perf_event_attr`], which we then feed into the existing perf source.
//!
//! libpfm is **loaded at runtime** with `dlopen` (via the `libloading` crate), not linked
//! at build time. Consequences:
//! - the libpfm shared library must be present on the machine that *runs* the agent (only
//!   if the configuration requires to collect libpfm encoded counters);
//! - if it is missing, `pfm_events` fail with a clear error instead of the binary refusing
//!   to start.
//!
//! The library name/location can be overridden at runtime with the `ALUMET_LIBPFM_LIB`
//! environment variable (a .so name searched in the standard loader paths, or a full path).
//! Otherwise a list of common .so names is tried.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

use anyhow::{Context, anyhow};
use libloading::Library;
use perf_event::events::Event;
use perf_event_open_sys::bindings::perf_event_attr;

use crate::events::NamedPerfEvent;

// Privilege level requested from libpfm, from `perfmon/pfmlib.h` (PFM_PLM3 = user).
//
// NOTE: this does NOT decide what the plugin actually measures. `perf-event2`'s `Builder`
// forces `exclude_kernel`/`exclude_hv` *after* our `update_attrs`, so every event (generic,
// libpfm or raw) is counted at the **user level only** (see `source.rs`). We pass `PFM_PLM3`
// so libpfm gets a valid mask that matches that reality; changing it here would have no
// effect on the collected values.
const PFM_PLM3: c_int = 0x08; // user
const PFM_SUCCESS: c_int = 0;

/// Environment variable to override the libpfm library name or full path at runtime.
const LIBPFM_ENV: &str = "ALUMET_LIBPFM_LIB";

/// ".so" names tried (in order) when `ALUMET_LIBPFM_LIB` is not set. The dynamic loader searches
/// the standard paths (`LD_LIBRARY_PATH`, `ld.so.cache`, default directories) for each.
const LIBPFM_CANDIDATES: &[&str] = &["libpfm.so.4", "libpfm.so"];

// Signatures of the three libpfm functions we call.
type PfmInitializeFn = unsafe extern "C" fn() -> c_int;
type PfmStrerrorFn = unsafe extern "C" fn(c_int) -> *const c_char;
type PfmGetEncodingFn =
    unsafe extern "C" fn(*const c_char, c_int, *mut perf_event_attr, *mut c_void, *mut c_int) -> c_int;

/// A handle to the dynamically-loaded libpfm, holding the function pointers we need.
struct LibPfm {
    // The library must stay alive as long as the function pointers are used. It lives in a
    // `OnceLock` for the whole program, so it is never unloaded.
    _lib: Library,
    strerror: PfmStrerrorFn,
    get_encoding: PfmGetEncodingFn,
}

// SAFETY: the raw function pointers point into `_lib`, which is kept alive alongside them.
unsafe impl Send for LibPfm {}
unsafe impl Sync for LibPfm {}

fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::from("(null)");
    }
    // SAFETY: libpfm returns a pointer to a static, NUL-terminated string.
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}

/// Open the libpfm shared library, resilient to a non-standard name or location.
///
/// Resolution order:
/// 1. `ALUMET_LIBPFM_LIB` — an explicit .so name (searched in the standard loader paths) or a
///    full path to the shared object. Covers both a different name and a different location.
/// 2. otherwise, the common .so names in [`LIBPFM_CANDIDATES`].
fn open_library() -> Result<Library, String> {
    if let Some(spec) = std::env::var_os(LIBPFM_ENV) {
        // SAFETY: opening a shared library runs its initializers; we trust libpfm.
        return unsafe { Library::new(&spec) }
            .map_err(|e| format!("cannot load libpfm from {LIBPFM_ENV}={}: {e}", spec.to_string_lossy()));
    }

    let mut last_err = String::new();
    for name in LIBPFM_CANDIDATES {
        // SAFETY: opening a shared library runs its initializers; we trust libpfm.
        match unsafe { Library::new(name) } {
            Ok(lib) => return Ok(lib),
            Err(e) => last_err = format!("{name}: {e}"),
        }
    }
    Err(format!(
        "cannot load libpfm (tried {LIBPFM_CANDIDATES:?}); \
         set {LIBPFM_ENV} to the library's .so name or full path. Last error: {last_err}"
    ))
}

/// Load libpfm once and initialize it. The result (success or failure) is cached for the
/// whole program lifetime.
fn libpfm() -> anyhow::Result<&'static LibPfm> {
    static LIBPFM: OnceLock<Result<LibPfm, String>> = OnceLock::new();

    let result = LIBPFM.get_or_init(|| {
        let lib = open_library()?;

        // SAFETY: the symbol signatures above match libpfm's C declarations.
        unsafe {
            let initialize: PfmInitializeFn = *lib
                .get::<PfmInitializeFn>(b"pfm_initialize\0")
                .map_err(|e| format!("symbol pfm_initialize not found: {e}"))?;
            let strerror: PfmStrerrorFn = *lib
                .get::<PfmStrerrorFn>(b"pfm_strerror\0")
                .map_err(|e| format!("symbol pfm_strerror not found: {e}"))?;
            let get_encoding: PfmGetEncodingFn = *lib
                .get::<PfmGetEncodingFn>(b"pfm_get_perf_event_encoding\0")
                .map_err(|e| format!("symbol pfm_get_perf_event_encoding not found: {e}"))?;

            let ret = initialize();
            if ret != PFM_SUCCESS {
                return Err(format!("pfm_initialize failed: {}", cstr_to_string(strerror(ret))));
            }
            Ok(LibPfm {
                _lib: lib,
                strerror,
                get_encoding,
            })
        }
    });

    result.as_ref().map_err(|e| anyhow!("{e}"))
}

/// A perf event whose encoding was resolved by libpfm.
///
/// Implements [`Event`] by copying the fields libpfm computed into the
/// `perf_event_attr`, so it plugs into the generic perf source builder like any
/// other event. Unlike [`perf_event::events::Raw`], it preserves `type` (libpfm may
/// return a dynamic PMU type rather than `PERF_TYPE_RAW`, e.g. for uncore events).
#[derive(Debug, Clone, Copy)]
pub struct PfmEvent {
    type_: u32,
    config: u64,
    config1: u64,
    config2: u64,
}

impl Event for PfmEvent {
    fn update_attrs(self, attr: &mut perf_event_attr) {
        attr.type_ = self.type_;
        attr.config = self.config;
        attr.config1 = self.config1;
        attr.config2 = self.config2;
    }
}

/// Returns a perf event from its libpfm name (e.g. `RESOURCE_STALLS:ANY`).
///
/// The name is resolved (and validated) against the current CPU via libpfm; the metric name
/// is derived from it with [`metric_suffix`].
pub fn parse_event(name: &str) -> anyhow::Result<NamedPerfEvent<PfmEvent>> {
    Ok(NamedPerfEvent {
        name: metric_suffix(name),
        description: format!("{name} (encoded via libpfm)"),
        event: encode(name)?,
    })
}

/// Resolve an event name (libpfm syntax, e.g. `RESOURCE_STALLS:ANY` or `INSTRUCTIONS:u`)
/// into a [`PfmEvent`]. Also validates that the event exists on the current CPU.
fn encode(name: &str) -> anyhow::Result<PfmEvent> {
    let lib = libpfm()?;

    let cname = CString::new(name).context("event name contains an interior null byte")?;
    let mut attr = perf_event_attr::default();

    // SAFETY: `cname` is a valid NUL-terminated string that outlives the call, and `attr`
    // is a valid, writable `perf_event_attr`. The optional outputs are passed as null.
    let ret = unsafe {
        (lib.get_encoding)(
            cname.as_ptr(),
            PFM_PLM3,
            &mut attr,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ret != PFM_SUCCESS {
        // SAFETY: `strerror` returns a static, NUL-terminated string.
        let msg = unsafe { cstr_to_string((lib.strerror)(ret)) };
        return Err(anyhow!(
            "libpfm cannot encode event '{name}': {msg}. \
             The event may not exist on this CPU, or the installed libpfm may be too old to know \
             this CPU model (it then falls back to the generic architectural PMU, which only exposes \
             basic events such as INSTRUCTIONS or CPU_CYCLES). \
             Check the exact name for your CPU with libpfm's `showevtinfo`."
        ));
    }

    Ok(PfmEvent {
        type_: attr.type_,
        config: attr.config,
        config1: attr.config1,
        config2: attr.config2,
    })
}

/// Turn a libpfm event name into a metric-name suffix, e.g. `RESOURCE_STALLS:ANY` ->
/// `RESOURCE_STALLS_ANY`. Case is preserved; non-alphanumeric characters become `_`.
fn metric_suffix(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perf_event_open_sys::bindings::perf_event_attr;

    #[test]
    fn metric_suffix_sanitizes() {
        assert_eq!(metric_suffix("RESOURCE_STALLS:ANY"), "RESOURCE_STALLS_ANY");
        assert_eq!(metric_suffix("INSTRUCTIONS:u"), "INSTRUCTIONS_u");
    }

    #[test]
    fn encode_generic_event() {
        // The generic perf PMU events are available on every CPU that libpfm supports.
        let event = encode("PERF_COUNT_HW_INSTRUCTIONS").expect("should encode a generic event");
        // It should actually configure the attr when applied.
        let mut attr = perf_event_attr::default();
        event.update_attrs(&mut attr);
        assert_eq!(attr.config, event.config);
    }

    #[test]
    fn encode_unknown_event_errors() {
        assert!(encode("DEFINITELY_NOT_A_REAL_EVENT_XYZ").is_err());
    }
}
