use tokio::sync::oneshot;

use crate::pipeline::{
    control::messages,
    elements::source::{
        builder::{AutonomousSourceBuilder, ManagedSource, ManagedSourceBuilder, SendSourceBuilder},
        control::CreateManyMessage,
        trigger::TriggerSpec,
    },
    naming::{PluginName, SourceName},
    Source,
};

use super::DirectResponseReceiver;

#[derive(Default, Debug)]
pub struct MultiCreationRequestBuilder {
    sources: Vec<(String, SendSourceBuilder)>,
    // TODO transforms and outputs when it becomes possible to add them at runtime
    // transforms: Vec<(String, Box<dyn TransformBuilder + Send>)>,
    // outputs: Vec<(String, SendOutputBuilder)>,
}

pub struct SingleCreationRequestBuilder {
    inner: MultiCreationRequestBuilder,
}

#[derive(Debug)]
pub struct CreationRequest {
    builders: MultiCreationRequestBuilder,
}

/// Returns a builder that allows to create multiple elements.
pub fn create_many() -> MultiCreationRequestBuilder {
    MultiCreationRequestBuilder::default()
}

/// Returns a builder that allows to create a single element.
pub fn create_one() -> SingleCreationRequestBuilder {
    SingleCreationRequestBuilder {
        inner: MultiCreationRequestBuilder::default(),
    }
}

impl SingleCreationRequestBuilder {
    /// Requests the creation of a (managed) measurement source.
    ///
    /// The source will be triggered according to the `trigger` specification.
    pub fn add_source(mut self, name: &str, source: Box<dyn Source>, trigger: TriggerSpec) -> CreationRequest {
        self.inner.add_source(name, source, trigger);
        self.inner.build()
    }

    pub fn add_source_builder<F>(mut self, name: &str, builder: F) -> CreationRequest
    where
        F: ManagedSourceBuilder + Send + 'static,
    {
        self.inner.add_source_builder(name, builder);
        self.inner.build()
    }

    pub fn add_autonomous_source_builder<F>(mut self, name: &str, builder: F) -> CreationRequest
    where
        F: AutonomousSourceBuilder + Send + 'static,
    {
        self.inner.add_autonomous_source_builder(name, builder);
        self.inner.build()
    }
}

impl MultiCreationRequestBuilder {
    pub fn build(&mut self) -> CreationRequest {
        CreationRequest {
            builders: std::mem::take(self),
        }
    }

    pub fn add_source(&mut self, name: &str, source: Box<dyn Source>, trigger: TriggerSpec) -> &mut Self {
        // TODO what about the SourceKey?
        self.add_source_builder(name, |_| {
            Ok(ManagedSource {
                trigger_spec: trigger,
                source,
            })
        });
        self
    }

    pub fn add_source_builder<F>(&mut self, name: &str, builder: F) -> &mut Self
    where
        F: ManagedSourceBuilder + Send + 'static,
    {
        let builder = SendSourceBuilder::Managed(Box::new(builder));
        self.sources.push((name.to_string(), builder));
        self
    }

    pub fn add_autonomous_source_builder<F>(&mut self, name: &str, builder: F) -> &mut Self
    where
        F: AutonomousSourceBuilder + Send + 'static,
    {
        let builder = SendSourceBuilder::Autonomous(Box::new(builder));
        self.sources.push((name.to_string(), builder));
        self
    }
}

impl CreationRequest {
    fn into_body(self, plugin: &PluginName) -> messages::EmptyResponseBody {
        let builders = self.builders;

        // add the plugin name to every builder
        let source_builders = builders
            .sources
            .into_iter()
            .map(|(source_name, builder)| {
                let full_name = SourceName::new(plugin.to_owned().0, source_name);
                (full_name, builder)
            })
            .collect();
        // create the message
        messages::EmptyResponseBody::Source(crate::pipeline::elements::source::control::ControlMessage::CreateMany(
            CreateManyMessage {
                builders: source_builders,
            },
        ))
    }
}

impl super::PluginControlRequest for CreationRequest {
    type OkResponse = ();
    type Receiver = DirectResponseReceiver<()>;

    fn serialize(self, plugin: &PluginName) -> messages::ControlRequest {
        messages::ControlRequest::NoResult(messages::RequestMessage {
            response_tx: None,
            body: self.into_body(plugin),
        })
    }

    fn serialize_with_response(self, plugin: &PluginName) -> (messages::ControlRequest, Self::Receiver) {
        let (tx, rx) = oneshot::channel();
        let req = messages::ControlRequest::NoResult(messages::RequestMessage {
            response_tx: Some(tx),
            body: self.into_body(plugin),
        });
        (req, DirectResponseReceiver(rx))
    }
}
