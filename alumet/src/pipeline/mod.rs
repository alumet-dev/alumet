//! Asynchronous and modular measurement pipeline.
//!
//! NOTE: If you are building an _agent_, you probably want to use the [`agent`](super::agent) module
//! instead of managing a pipeline manually.
//!
//! # Pipeline lifecycle
//! 1. Create a new [`Builder`].
//! 2. Call builder's methods to set pipeline options and to register pipeline elements (sources, transforms, outputs).
//! 3. Build and start the pipeline with [`Builder::build()`].
//! 4. Stop the pipeline by calling [`pipeline.control_handle().shutdown()`](control::AnonymousControlHandle::shutdown).
//! 5. Finalize the shutdown with [`pipeline.wait_for_shutdown()`](MeasurementPipeline::wait_for_shutdown).

pub mod builder;
pub mod control;
pub mod elements;
pub mod naming;
pub mod trigger;
pub(crate) mod util;

pub use elements::output::Output;
pub use elements::source::Source;
pub use elements::transform::Transform;

pub use builder::Builder;
pub use builder::MeasurementPipeline;
pub use util::matching;
pub use util::naming::{ElementKind, PluginName};
