//! Control requests: change the configuration of the measurement pipeline.
//!
//! # Examples
//! ## Disabling a source
//!
//! ```no_run
//! use std::time::Duration;
//! use alumet::pipeline::control::{request, PluginControlHandle, key::SourceKey};
//!
//! async fn example() {
//!     let control_handle: PluginControlHandle = todo!();
//!     let source_key: SourceKey = todo!();
//!     let req = request::source(source_key).disable();
//!     let timeout = Duration::from_secs(1);
//!     control_handle.send_wait(req, timeout).await;
//! }
//! ```

use crate::pipeline::naming::PluginName;

use super::main_loop::ControlRequestBody;

mod create;
mod output;
mod public;
mod source;
mod transform;

pub use create::{create_many, create_one, CreationRequest, MultiCreationRequestBuilder, SingleCreationRequestBuilder};
pub use output::{output, RemainingDataStrategy};
pub use public::*;
pub use source::source;
pub use transform::transform;

/// A request for the pipeline controller.
pub(super) trait ControlRequest {
    fn serialize(self) -> ControlRequestBody;
}

/// A control request that requires a `PluginName`.
pub(super) trait PluginControlRequest {
    fn serialize(self, plugin: &PluginName) -> ControlRequestBody;
}

// Every ControlRequest is a PluginControlRequest!
impl<R: ControlRequest> PluginControlRequest for R {
    fn serialize(self, _plugin: &PluginName) -> ControlRequestBody {
        ControlRequest::serialize(self)
    }
}
