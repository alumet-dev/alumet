//! Public opaque types for requests.

use crate::pipeline::{
    control::{messages, request::RecvError},
    error::PipelineError,
    naming::PluginName,
};

use super::{
    AnonymousControlRequest, CreationRequest, DirectResponseReceiver, PluginControlRequest, ResponseReceiver, create,
    introspect::IntrospectionRequest, output::OutputRequest, source::SourceRequest, transform::TransformRequest,
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
    Introspect(IntrospectionRequest),
}

#[derive(Debug)]
enum PluginControlRequestImpl {
    Anonymous(AnyAnonymousControlRequest),
    Create(create::CreationRequest),
}

/// A `ResponseReceiver` that replaces the response by `()`.
///
/// It can encapsulate any `ResponseReceiver`.
pub struct ResponseDiscarder(ResponseDiscarderImpl);

// `ResponseReceiver` is not dyn-compatible, we have to use the concrete type
// `DirectResponseReceiver<R>` and specify `R`.
enum ResponseDiscarderImpl {
    NoResult(DirectResponseReceiver<()>),
    Introspect(DirectResponseReceiver<messages::IntrospectionResponse>),
}

impl From<DirectResponseReceiver<()>> for ResponseDiscarder {
    fn from(value: DirectResponseReceiver<()>) -> Self {
        Self(ResponseDiscarderImpl::NoResult(value))
    }
}

impl From<DirectResponseReceiver<messages::IntrospectionResponse>> for ResponseDiscarder {
    fn from(value: DirectResponseReceiver<messages::IntrospectionResponse>) -> Self {
        Self(ResponseDiscarderImpl::Introspect(value))
    }
}

impl ResponseReceiver for ResponseDiscarder {
    type Ok = ();

    async fn recv(self) -> Result<Result<Self::Ok, PipelineError>, RecvError> {
        fn discard_success<R>(
            res: Result<Result<R, PipelineError>, RecvError>,
        ) -> Result<Result<(), PipelineError>, RecvError> {
            match res {
                Ok(Ok(_)) => Ok(Ok(())), // discard the response
                Ok(Err(e)) => Ok(Err(e)),
                Err(e) => Err(e),
            }
        }

        match self.0 {
            ResponseDiscarderImpl::NoResult(r) => discard_success(r.recv().await),
            ResponseDiscarderImpl::Introspect(r) => discard_success(r.recv().await),
        }
    }
}

// trait implementations
impl AnonymousControlRequest for AnyAnonymousControlRequest {
    type OkResponse = ();
    type Receiver = ResponseDiscarder;

    fn serialize(self) -> crate::pipeline::control::messages::ControlRequest {
        match self.0 {
            ControlRequestImpl::Output(req) => AnonymousControlRequest::serialize(req),
            ControlRequestImpl::Source(req) => AnonymousControlRequest::serialize(req),
            ControlRequestImpl::Transform(req) => AnonymousControlRequest::serialize(req),
            ControlRequestImpl::Introspect(req) => AnonymousControlRequest::serialize(req),
        }
    }

    fn serialize_with_response(self) -> (crate::pipeline::control::messages::ControlRequest, ResponseDiscarder) {
        match self.0 {
            ControlRequestImpl::Output(req) => {
                let (req, rx) = AnonymousControlRequest::serialize_with_response(req);
                (req, ResponseDiscarder::from(rx))
            }
            ControlRequestImpl::Source(req) => {
                let (req, rx) = AnonymousControlRequest::serialize_with_response(req);
                (req, ResponseDiscarder::from(rx))
            }
            ControlRequestImpl::Transform(req) => {
                let (req, rx) = AnonymousControlRequest::serialize_with_response(req);
                (req, ResponseDiscarder::from(rx))
            }
            ControlRequestImpl::Introspect(req) => {
                let (req, rx) = AnonymousControlRequest::serialize_with_response(req);
                (req, ResponseDiscarder::from(rx))
            }
        }
    }
}

impl PluginControlRequest for AnyPluginControlRequest {
    type OkResponse = ();
    type Receiver = ResponseDiscarder;

    fn serialize(self, plugin: &PluginName) -> messages::ControlRequest {
        match self.0 {
            PluginControlRequestImpl::Anonymous(req) => PluginControlRequest::serialize(req, plugin),
            PluginControlRequestImpl::Create(req) => PluginControlRequest::serialize(req, plugin),
        }
    }

    fn serialize_with_response(self, plugin: &PluginName) -> (messages::ControlRequest, ResponseDiscarder) {
        match self.0 {
            PluginControlRequestImpl::Anonymous(req) => {
                let (req, rx) = PluginControlRequest::serialize_with_response(req, plugin);
                (req, ResponseDiscarder::from(rx))
            }
            PluginControlRequestImpl::Create(req) => {
                let (req, rx) = PluginControlRequest::serialize_with_response(req, plugin);
                (req, ResponseDiscarder::from(rx))
            }
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
impl From<IntrospectionRequest> for AnyAnonymousControlRequest {
    fn from(value: IntrospectionRequest) -> Self {
        Self(ControlRequestImpl::Introspect(value))
    }
}

impl From<AnyAnonymousControlRequest> for AnyPluginControlRequest {
    fn from(value: AnyAnonymousControlRequest) -> Self {
        Self(PluginControlRequestImpl::Anonymous(value))
    }
}
impl From<CreationRequest> for AnyPluginControlRequest {
    fn from(value: CreationRequest) -> Self {
        Self(PluginControlRequestImpl::Create(value))
    }
}
