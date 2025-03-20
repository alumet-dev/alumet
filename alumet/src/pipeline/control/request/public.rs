//! Public opaque types for requests.

use crate::pipeline::{control::main_loop::ControlRequestBody, naming::PluginName};

use super::{
    create, output::OutputRequest, source::SourceRequest, transform::TransformRequest, ControlRequest,
    PluginControlRequest,
};

#[derive(Debug)]
pub struct AnyAnonymousControlRequest(ControlRequestImpl);
#[derive(Debug)]
pub struct AnyPluginControlRequest(PluginControlRequestImpl);

#[derive(Debug)]
enum ControlRequestImpl {
    Output(OutputRequest),
    Source(SourceRequest),
    Transform(TransformRequest),
}

#[derive(Debug)]
enum PluginControlRequestImpl {
    Anonymous(AnyAnonymousControlRequest),
    Create(create::CreationRequest),
}

// trait implementations

impl ControlRequest for AnyAnonymousControlRequest {
    fn serialize(self) -> ControlRequestBody {
        match self.0 {
            ControlRequestImpl::Output(req) => ControlRequest::serialize(req),
            ControlRequestImpl::Source(req) => ControlRequest::serialize(req),
            ControlRequestImpl::Transform(req) => ControlRequest::serialize(req),
        }
    }
}

impl PluginControlRequest for AnyPluginControlRequest {
    fn serialize(self, plugin: &PluginName) -> ControlRequestBody {
        match self.0 {
            PluginControlRequestImpl::Create(req) => PluginControlRequest::serialize(req, plugin),
            PluginControlRequestImpl::Anonymous(req) => ControlRequest::serialize(req),
        }
    }
}

// conversion/construction
impl From<SourceRequest> for AnyAnonymousControlRequest {
    fn from(value: SourceRequest) -> Self {
        Self(ControlRequestImpl::Source(value))
    }
}
impl From<TransformRequest> for AnyAnonymousControlRequest {
    fn from(value: TransformRequest) -> Self {
        Self(ControlRequestImpl::Transform(value))
    }
}
impl From<OutputRequest> for AnyAnonymousControlRequest {
    fn from(value: OutputRequest) -> Self {
        Self(ControlRequestImpl::Output(value))
    }
}
