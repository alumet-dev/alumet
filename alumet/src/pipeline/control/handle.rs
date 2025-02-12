use tokio::sync::mpsc::{self, Sender};
use tokio_util::sync::CancellationToken;

use crate::pipeline::{
    elements::{
        output,
        source::{self, builder::ManagedSourceBuilder},
        transform,
    },
    naming::{PluginName, SourceName},
    trigger, Source,
};

use super::{
    error::{ControlError, ControlSendError},
    SourceCreationBuffer,
};

/// A control handle that is not tied to a particular plugin.
///
/// Unlike [`ScopedControlHandle`], `AnonymousControlHandle` does not provide any method
/// that register new pipeline elements. You can call [`AnonymousControlHandle::scoped`] to turn an anonymous handle
/// into a scoped one.
#[derive(Clone)]
pub struct AnonymousControlHandle {
    tx: Sender<ControlMessage>,
    shutdown: CancellationToken,
}

/// A control handle with the scope of a plugin.
///
/// Sources registered with methods like [`ScopedControlHandle::add_source`] will be named after the plugin scope.
#[derive(Clone)]
pub struct ScopedControlHandle {
    pub(super) inner: AnonymousControlHandle,
    pub(super) plugin: PluginName,
}

/// A message that can be sent "to the pipeline" (I'm simplifying here) in order to control it.
#[derive(Debug)]
pub enum ControlMessage {
    Source(source::control::ControlMessage),
    Transform(transform::control::ControlMessage),
    Output(output::control::ControlMessage),
}

#[cfg(test)]
mod tests {
    use crate::pipeline::util;

    use super::ControlMessage;

    #[test]
    fn type_constraints() {
        util::assert_send::<ControlMessage>();
    }
}

impl AnonymousControlHandle {
    pub(super) fn new(tx: Sender<ControlMessage>, shutdown: CancellationToken) -> Self {
        Self { tx, shutdown }
    }

    /// Sends a control message to the pipeline, waiting until there is capacity.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline has been shut down.
    pub async fn send(&self, message: ControlMessage) -> Result<(), ControlError> {
        self.tx.send(message).await.map_err(|_| ControlError::Shutdown)
    }

    /// Attempts to immediately send a control message to the pipeline.
    ///
    /// # Errors
    ///
    /// There are two possible cases:
    /// - The pipeline has been shut down and can no longer accept any message.
    /// - The buffer of the control channel is full.
    pub fn try_send(&self, message: ControlMessage) -> Result<(), ControlSendError> {
        match self.tx.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(m)) => Err(ControlSendError::ChannelFull(m)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ControlSendError::Shutdown),
        }
    }

    /// Requests the pipeline to shut down.
    pub fn shutdown(&self) {
        self.shutdown.cancel()
    }

    /// Creates a new handle with the given plugin scope.
    pub fn scoped(&self, plugin: PluginName) -> ScopedControlHandle {
        ScopedControlHandle {
            inner: self.clone(),
            plugin,
        }
    }
}

impl ScopedControlHandle {
    pub fn anonymous(&self) -> &AnonymousControlHandle {
        &self.inner
    }

    /// Creates a new buffer for bulk source creation.
    pub fn source_buffer<'a>(&'a mut self) -> SourceCreationBuffer<'a> {
        SourceCreationBuffer {
            handle: self,
            buffer: Vec::new(),
        }
    }

    /// Creates a new buffer for bulk source creation, with the given initial capacity.
    pub fn source_buffer_with_capacity<'a>(&'a mut self, capacity: usize) -> SourceCreationBuffer<'a> {
        SourceCreationBuffer {
            handle: self,
            buffer: Vec::with_capacity(capacity),
        }
    }

    /// Adds a measurement source to the Alumet pipeline.
    ///
    /// This is similar to [`AlumetPluginStart::add_source()`](crate::plugin::AlumetPluginStart::add_source()).
    ///
    /// # Bulk registration of sources
    /// To add multiple sources, it is more efficient to use [`source_buffer`](Self::source_buffer) instead of many `add_source`.
    pub fn add_source(
        &self,
        name: &str,
        source: Box<dyn Source>,
        trigger: trigger::TriggerSpec,
    ) -> Result<(), ControlError> {
        let builder = self.managed_source_builder(trigger, source);
        self.add_source_builder(name, builder)
    }

    /// Adds a measurement source to the Alumet pipeline, with an explicit builder.
    ///
    /// This is similar to [`AlumetPluginStart::add_source_builder()`](crate::plugin::AlumetPluginStart::add_source_builder()),
    /// except that the builder needs to be [`Send`].
    ///
    /// # Bulk registration of sources
    /// To add multiple sources, it is more efficient to use [`source_buffer`](Self::source_buffer) instead of many `add_source_builder`.
    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(
        &self,
        name: &str,
        builder: F,
    ) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::control::ControlMessage::CreateOne(
            source::control::CreateOneMessage {
                name: SourceName::new(self.plugin.0.clone(), name.to_owned()),
                builder: source::builder::SendSourceBuilder::Managed(Box::new(builder)),
            },
        ));
        self.inner.try_send(message).map_err(|e| e.into())
    }

    /// Adds an autonomous measurement source to the Alumet pipeline, with an explicit builder.
    ///
    /// This is similar to [`AlumetPluginStart::add_autonomous_source_builder()`](crate::plugin::AlumetPluginStart::add_autonomous_source_builder()),
    /// except that the builder needs to be [`Send`].
    ///
    /// # Bulk registration of sources
    /// To add multiple sources, it is more efficient to use [`source_buffer`](Self::source_buffer) instead of many `add_source_builder`.
    pub fn add_autonomous_source_builder<F: source::builder::AutonomousSourceBuilder + Send + 'static>(
        &self,
        name: &str,
        builder: F,
    ) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::control::ControlMessage::CreateOne(
            source::control::CreateOneMessage {
                name: SourceName::new(self.plugin.0.clone(), name.to_owned()),
                builder: source::builder::SendSourceBuilder::Autonomous(Box::new(builder)),
            },
        ));
        self.inner.try_send(message).map_err(|e| e.into())
    }

    /// Returns a source builder that returns the given boxed source.
    pub(super) fn managed_source_builder(
        &self,
        trigger: trigger::TriggerSpec,
        source: Box<dyn Source>,
    ) -> impl FnOnce(&mut dyn source::builder::ManagedSourceBuildContext) -> anyhow::Result<source::builder::ManagedSource>
    {
        move |_ctx: &mut dyn source::builder::ManagedSourceBuildContext| {
            Ok(source::builder::ManagedSource {
                trigger_spec: trigger,
                source,
            })
        }
    }
}
