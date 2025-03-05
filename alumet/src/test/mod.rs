//! Testing utilities.
//!
//! # Rationale
//!
//! Alumet is a modular measurement framework and provides a plugin API.
//! With this API, plugins can register new metrics, create sources, generate measurements,
//! etc. All these things should be tested!
//!
//! However, since plugins call methods on Alumet structures, and rely on the Alumet framework
//! to function, they are not well suited for independent unit tests.
//! One could think about mocking the framework's structs, but it would be both unpractical
//! and fragile, because plugins rely on the behavior of the framework.
//!
//! This module provides a solution: a set of utilities that allow you to declare tests
//! to apply to plugins and the elements they register (sources, transforms, outputs).
//!
//! # Feature Flag
//!
//! To use this module, you need to enable the `test` feature of Alumet.
//! Since you only need it for testing, the feature should only be enabled in `dev-dependencies`.
//!
//! Extract of `Cargo.toml`:
//! ```toml
//! [dependencies]
//! alumet = "version"
//!
//! [dev-dependencies]
//! alumet = {version = "version", features = ["test"]}
//! ```

/// Tests performed while the measurement pipeline is running.
pub mod runtime;

/// Tests performed at startup.
pub mod startup;

pub use runtime::RuntimeExpectations;
pub use startup::StartupExpectations;
