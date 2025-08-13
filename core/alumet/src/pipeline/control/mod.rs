//! On-the-fly modification of the pipeline.
pub mod handle;
pub mod key;
mod main_loop;
pub mod matching;
mod messages;
pub mod request;

pub use handle::{AnonymousControlHandle, PluginControlHandle};
pub(crate) use main_loop::PipelineControl;
