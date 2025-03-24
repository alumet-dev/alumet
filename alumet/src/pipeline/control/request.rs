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

use crate::pipeline::{error::PipelineError, naming::PluginName};

pub mod any;
mod create;
pub(super) mod introspect;
mod output;
mod source;
mod transform;

pub use create::{create_many, create_one, CreationRequest, MultiCreationRequestBuilder, SingleCreationRequestBuilder};
pub use introspect::{list_elements, ElementListFilter, IntrospectionRequest};
pub use output::{output, OutputRequest, OutputRequestBuilder, RemainingDataStrategy};
pub use source::{source, SourceRequest, SourceRequestBuilder};
use tokio::sync::oneshot;
pub use transform::{transform, TransformRequest, TransformRequestBuilder};

use super::messages;

/// An anonymous request for the pipeline controller.
pub(super) trait AnonymousControlRequest {
    /// Type of response (on success).
    type OkResponse;

    /// Type of receiver.
    type Receiver: ResponseReceiver<Ok = Self::OkResponse>;

    fn serialize(self) -> messages::ControlRequest;
    fn serialize_with_response(self) -> (messages::ControlRequest, Self::Receiver);
}

/// A control request that requires a `PluginName`.
pub(super) trait PluginControlRequest {
    /// Type of response (on success).
    type OkResponse;

    /// Type of receiver.
    type Receiver: ResponseReceiver<Ok = Self::OkResponse>;

    fn serialize(self, plugin: &PluginName) -> messages::ControlRequest;
    fn serialize_with_response(self, plugin: &PluginName) -> (messages::ControlRequest, Self::Receiver);
}

// Every AnonymousControlRequest is a PluginControlRequest!
impl<R: AnonymousControlRequest> PluginControlRequest for R {
    type OkResponse = R::OkResponse;
    type Receiver = R::Receiver;

    fn serialize(self, _plugin: &PluginName) -> messages::ControlRequest {
        AnonymousControlRequest::serialize(self)
    }

    fn serialize_with_response(self, _plugin: &PluginName) -> (messages::ControlRequest, Self::Receiver) {
        AnonymousControlRequest::serialize_with_response(self)
    }
}

pub(super) trait ResponseReceiver {
    type Ok;

    async fn recv(self) -> Result<Result<Self::Ok, PipelineError>, RecvError>;
}

pub(super) struct RecvError;

pub(super) struct DirectResponseReceiver<R>(oneshot::Receiver<Result<R, PipelineError>>);

impl<R> ResponseReceiver for DirectResponseReceiver<R> {
    type Ok = R;

    async fn recv(self) -> Result<Result<Self::Ok, PipelineError>, RecvError> {
        self.0.await.map_err(|_| RecvError)
    }
}

impl<R> From<oneshot::Receiver<Result<R, PipelineError>>> for DirectResponseReceiver<R> {
    fn from(value: oneshot::Receiver<Result<R, PipelineError>>) -> Self {
        Self(value)
    }
}
