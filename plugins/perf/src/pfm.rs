//! Support for events encoded via libpfm4.
//!
//! The native events exposed by the kernel are a small, vendor-neutral subset.
//! CPUs also expose hundreds of PMU events whose encodings differ per microarchitecture.
//! [libpfm4](https://perfmon2.sourceforge.net/) holds those encoding tables.
//! Given a human-readable name (e.g. `RESOURCE_STALLS:ANY`) it fills a [`perf_event_attr`], 
//! which we then feed into the existing perf source.
//!
//! libpfm is **loaded at runtime** with `dlopen` (via the `libloading` crate), not linked
//! at build time. Consequences:
//! - the libpfm shared library must be present on the machine that *runs* the agent (only
//!   if the configuration requires to collect libpfm encoded counters);
//! - if it is missing, a clear error is showed and make the Alumet agent to crash.
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

// Privilege level requested from libpfm.
//
// NOTE: this does NOT decide what the plugin actually measures as the domain is chosen by the
// plugin's own modifiers. This will be overriden so changing the value here doesn't change the privileges.
const PFM_PLM3: c_int = 0x08; // user
const PFM_SUCCESS: c_int = 0;

/// `pfm_os_t` value selecting the perf_events encoding (from `pfmlib.h`).
///
/// We use the base `PFM_OS_PERF_EVENT`, not `PFM_OS_PERF_EVENT_EXT`. Both produce the same event
/// encoding (`type`/`config`/`config1`/`config2`). `_EXT` also lets the event *string* carry perf 
/// sampling attributes (`:period=`, `:freq=`, `:precise=`, etc...) that it writes into other `perf_event_attr`
/// fields. This plugin is a counter (not a sampler) and owns the domain modifiers itself, so those attributes bring nothing.
const PFM_OS_PERF_EVENT: c_int = 1;

/// Environment variable to override the libpfm library name or full path at runtime.
const LIBPFM_ENV: &str = "ALUMET_LIBPFM_LIB";

/// ".so" names tried when `ALUMET_LIBPFM_LIB` is not set. The dynamic loader searches
/// the standard paths (`LD_LIBRARY_PATH`, `ld.so.cache`, default directories) for each.
const LIBPFM_CANDIDATES: &[&str] = &["libpfm.so.4", "libpfm.so"];

// Signatures of the three libpfm functions we call.
type PfmInitializeFn = unsafe extern "C" fn() -> c_int;
type PfmStrerrorFn = unsafe extern "C" fn(c_int) -> *const c_char;
type PfmGetOsEncodingFn = unsafe extern "C" fn(*const c_char, c_int, c_int, *mut c_void) -> c_int;

/// Mirror of libpfm's `pfm_perf_encode_arg_t` (from `pfmlib_perf_event.h`): the `output` argument
/// of `pfm_get_os_event_encoding` when the OS is [`PFM_OS_PERF_EVENT`]. libpfm fills `attr`
/// (the only field we read back); the others are outputs we don't use.
#[repr(C)]
struct PfmPerfEncodeArg {
    attr: *mut perf_event_attr,
    fstr: *mut *mut c_char,
    size: usize,
    idx: c_int,
    cpu: c_int,
    flags: c_int,
    pad0: c_int,
}

// libpfm validates `arg.size` against its `PFM_PERF_ENCODE_ABI0` (40 on 64-bit, 28 on 32-bit), so
// our mirror must have exactly that size.
const _: () = assert!(
    std::mem::size_of::<PfmPerfEncodeArg>() == if cfg!(target_pointer_width = "64") { 40 } else { 28 }
);

/// A handle to the dynamically-loaded libpfm, holding the function pointers we need.
struct LibPfm {
    // The library must stay alive as long as the function pointers are used. It lives in a
    // `OnceLock` for the whole program, so it is never unloaded.
    _lib: Library,
    strerror: PfmStrerrorFn,
    get_encoding: PfmGetOsEncodingFn,
}

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
/// 1. `ALUMET_LIBPFM_LIB` : an explicit .so name (searched in the standard loader paths) or a
///    full path to the shared object. Covers both a different name and a different location.
/// 2. otherwise, the common .so names in [`LIBPFM_CANDIDATES`].
fn open_library() -> anyhow::Result<Library> {
    if let Some(spec) = std::env::var_os(LIBPFM_ENV) {
        // SAFETY: opening a shared library runs its initializers; we trust libpfm.
        return unsafe { Library::new(&spec) }
            .with_context(|| format!("cannot load libpfm from {LIBPFM_ENV}={}", spec.to_string_lossy()));
    }

    let mut last_err = String::new();
    for name in LIBPFM_CANDIDATES {
        // SAFETY: opening a shared library runs its initializers; we trust libpfm.
        match unsafe { Library::new(name) } {
            Ok(lib) => return Ok(lib),
            Err(e) => last_err = format!("{name}: {e}"),
        }
    }
    Err(anyhow!(
        "cannot load libpfm (tried {LIBPFM_CANDIDATES:?}); \
         set {LIBPFM_ENV} to the library's .so name or full path. Last error: {last_err}"
    ))
}

/// Load libpfm once and initialize it. The result is cached for the
/// whole program lifetime.
fn libpfm() -> anyhow::Result<&'static LibPfm> {
    static LIBPFM: OnceLock<anyhow::Result<LibPfm>> = OnceLock::new();

    let result = LIBPFM.get_or_init(|| {
        let lib = open_library()?;

        // SAFETY: the symbol signatures above match libpfm's C declarations.
        unsafe {
            let initialize: PfmInitializeFn = *lib
                .get::<PfmInitializeFn>(b"pfm_initialize\0")
                .with_context(|| format!("symbol pfm_initialize not found"))?;
            let strerror: PfmStrerrorFn = *lib
                .get::<PfmStrerrorFn>(b"pfm_strerror\0")
                .with_context(|| format!("symbol pfm_strerror not found"))?;
            let get_encoding: PfmGetOsEncodingFn = *lib
                .get::<PfmGetOsEncodingFn>(b"pfm_get_os_event_encoding\0")
                .with_context(|| format!("symbol pfm_get_os_event_encoding not found"))?;

            let ret = initialize();
            if ret != PFM_SUCCESS {
                return Err(anyhow!("pfm_initialize failed: {}", cstr_to_string(strerror(ret))));
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

/// Resolve an event name (libpfm syntax, e.g. `RESOURCE_STALLS:ANY`) into a [`PfmEvent`].
/// Also validates that the event exists on the current CPU.
/// A libpfm event name is structured like this (parsing is case-insensitive):
///
///`[pmu::]event_name[:unit_mask][:unit_mask…]`
///
///- **`pmu::`** *(optional)* : a libpfm PMU / microarchitecture model, e.g. `ix86arch::`, to
///  disambiguate an event. Usually unnecessary : libpfm auto-detects your CPU.
///- **`event_name`** *(required)* : the full event name, e.g. `RESOURCE_STALLS`.
///- **`:unit_mask`** *(optional, repeatable)* : a sub-event that refines the event, e.g. `:ANY` or
///  `:L3_MISS`. Some events require one; some accept several.
pub fn encode(name: &str) -> anyhow::Result<PfmEvent> {
    let lib = libpfm()?;

    let cname = CString::new(name).context("event name contains an interior null byte")?;
    let mut attr = perf_event_attr::default();

    let mut arg = PfmPerfEncodeArg {
        attr: &mut attr,
        fstr: std::ptr::null_mut(),
        size: std::mem::size_of::<PfmPerfEncodeArg>(),
        idx: 0,
        cpu: 0,
        flags: 0,
        pad0: 0,
    };

    // SAFETY: `cname` is a valid NUL-terminated string that outlives the call; `arg` (and the
    // `attr` it points to) are valid and writable for the duration of the call.
    let ret = unsafe {
        (lib.get_encoding)(
            cname.as_ptr(),
            PFM_PLM3,
            PFM_OS_PERF_EVENT,
            std::ptr::from_mut(&mut arg).cast::<c_void>(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_generic_event() {
        use perf_event_open_sys::bindings::{PERF_COUNT_HW_INSTRUCTIONS, PERF_TYPE_HARDWARE};

        // Requires libpfm at runtime; skip cleanly where it is not installed.
        if libpfm().is_err() {
            eprintln!("skipping encode_generic_event: libpfm is not available");
            return;
        }
        let event = encode_raw("PERF_COUNT_HW_INSTRUCTIONS").expect("should encode a generic event");

        // It must be the generic hardware "instructions" event, with no extra config words.
        assert_eq!(event.type_, PERF_TYPE_HARDWARE);
        assert_eq!(event.config, u64::from(PERF_COUNT_HW_INSTRUCTIONS));
        assert_eq!(event.config1, 0);
        assert_eq!(event.config2, 0);

        // Applying it must write exactly those four fields onto a fresh attr.
        let mut attr = perf_event_attr::default();
        event.update_attrs(&mut attr);
        assert_eq!(attr.type_, PERF_TYPE_HARDWARE);
        assert_eq!(attr.config, u64::from(PERF_COUNT_HW_INSTRUCTIONS));
        assert_eq!(attr.config1, 0);
        assert_eq!(attr.config2, 0);
    }

    #[test]
    fn encode_unknown_event_errors() {
        // Errors whether libpfm is missing (load failure) or present (unknown event). 
        assert!(encode("DEFINITELY_NOT_A_REAL_EVENT_XYZ").is_err());
    }
}
