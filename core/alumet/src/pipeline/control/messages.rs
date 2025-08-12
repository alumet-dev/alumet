use tokio::sync::{mpsc, oneshot};

use crate::pipeline::{
    elements::{output, source, transform},
    error::PipelineError,
    matching::ElementNamePattern,
    naming::ElementName,
};

pub type Receiver = mpsc::Receiver<ControlRequest>;
pub type Sender = mpsc::Sender<ControlRequest>;

#[derive(Debug)]
pub enum ControlRequest {
    NoResult(RequestMessage<EmptyResponseBody, ()>),
    Introspect(RequestMessage<IntrospectionBody, IntrospectionResponse>),
}

pub type ResponseSender<R> = oneshot::Sender<Result<R, PipelineError>>;

#[derive(Debug)]
pub struct RequestMessage<Body, Response> {
    pub(super) response_tx: Option<ResponseSender<Response>>,
    pub(super) body: Body,
}

#[derive(Debug)]
pub enum EmptyResponseBody {
    Single(SpecificBody),
    Mixed(Vec<SpecificBody>),
}

#[derive(Debug)]
pub enum SpecificBody {
    Source(source::control::ControlMessage),
    Transform(transform::control::ControlMessage),
    Output(output::control::ControlMessage),
}

#[derive(Debug)]
pub enum IntrospectionBody {
    ListElements(ElementNamePattern),
}

pub type IntrospectionResponse = Vec<ElementName>;
