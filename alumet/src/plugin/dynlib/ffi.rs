//! Foreign-Function interface for dynamically-loaded plugins.
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

// TODO