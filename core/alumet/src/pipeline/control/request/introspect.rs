use tokio::sync::oneshot;

use crate::pipeline::{
    control::messages,
    matching::{ElementNamePattern, StringPattern},
    naming::ElementKind,
};

use super::{AnonymousControlRequest, DirectResponseReceiver};

/// Creates a request that lists the elements of the pipeline that match the given filter.
pub fn list_elements(filter: ElementListFilter) -> IntrospectionRequest {
    IntrospectionRequest { list_filter: filter }
}

#[derive(Debug)]
pub struct IntrospectionRequest {
    list_filter: ElementListFilter,
}

#[derive(Debug)]
pub struct ElementListFilter {
    pub(crate) pattern: ElementNamePattern,
}

impl ElementListFilter {
    pub fn kind(kind: ElementKind) -> Self {
        Self::with_kind(Some(kind))
    }

    pub fn kind_any() -> Self {
        Self::with_kind(None)
    }

    fn with_kind(kind: Option<ElementKind>) -> Self {
        Self {
            pattern: ElementNamePattern {
                kind,
                plugin: StringPattern::Any,
                element: StringPattern::Any,
            },
        }
    }

    pub fn plugin(mut self, plugin: impl Into<String>) -> Self {
        self.pattern.plugin = StringPattern::Exact(plugin.into());
        self
    }

    pub fn plugin_pat(mut self, plugin: StringPattern) -> Self {
        self.pattern.plugin = plugin;
        self
    }

    pub fn name(mut self, element_name: impl Into<String>) -> Self {
        self.pattern.element = StringPattern::Exact(element_name.into());
        self
    }

    pub fn name_pat(mut self, element_name: StringPattern) -> Self {
        self.pattern.element = element_name;
        self
    }
}

impl IntrospectionRequest {
    fn into_body(self) -> messages::IntrospectionBody {
        messages::IntrospectionBody::ListElements(self.list_filter.pattern)
    }
}

impl AnonymousControlRequest for IntrospectionRequest {
    type OkResponse = messages::IntrospectionResponse;
    type Receiver = DirectResponseReceiver<Self::OkResponse>;

    fn serialize(self) -> messages::ControlRequest {
        messages::ControlRequest::Introspect(messages::RequestMessage {
            response_tx: None,
            body: self.into_body(),
        })
    }

    fn serialize_with_response(self) -> (messages::ControlRequest, Self::Receiver) {
        let (tx, rx) = oneshot::channel();
        let req = messages::ControlRequest::Introspect(messages::RequestMessage {
            response_tx: Some(tx),
            body: self.into_body(),
        });
        (req, DirectResponseReceiver(rx))
    }
}
