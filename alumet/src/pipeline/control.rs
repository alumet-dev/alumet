//! On-the-fly modification of the pipeline.
use super::builder::elements::{
    AutonomousSourceBuilder, ManagedSourceBuilder, ManagedSourceRegistration, SendSourceBuilder,
};
use super::elements::source::CreateManyMessage;
use super::elements::{output, source, transform};
use super::{builder, trigger, PluginName, Source};
use thiserror::Error;
use tokio::runtime;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
    inner: AnonymousControlHandle,
    plugin: PluginName,
}

/// A buffer that allows to create multiple sources with only one control message.
///
/// # Example
/// ```no_run
/// use alumet::pipeline::control::{ScopedControlHandle, SourceCreationBuffer};
/// use alumet::pipeline::Source;
///
/// // Assume that we have:
/// // - a `control_handle: ScopedControlHandle`.
/// // - a function `create_source()` that returns a `Box<dyn Source>`
///
/// # let control_handle: ScopedControlHandle = todo!();
/// # fn create_source() -> Box<dyn Source> { todo!() }
/// #
/// // create the buffer
/// let mut buf: SourceCreationBuffer<'_> = control_handle.source_buffer();
///
/// // add multiple sources, here 10
/// for i in 1..=10 {
///     let source = create_source();
///     let trigger = todo!();
///     buf.add_source(&format!("source-{i}"), source, trigger);
/// }
///
/// // register all the sources at once
/// buf.flush().expect("failed to create sources");
/// ```
///
/// # Flushing
/// The buffer is automatically flushed on drop, but **you should call flush**
/// in order to handle the errors. The drop implementation silently ignores flushing errors.
pub struct SourceCreationBuffer<'a> {
    inner: &'a mut ScopedControlHandle,
    buffer: Vec<builder::elements::SendSourceBuilder>,
}

/// A message that can be sent "to the pipeline" (I'm simplifying here) in order to control it.
#[derive(Debug)]
pub enum ControlMessage {
    Source(source::ControlMessage),
    Transform(transform::ControlMessage),
    Output(output::ControlMessage),
}

/// Encapsulates sources, transforms and outputs control.
pub(crate) struct PipelineControl {
    sources: source::SourceControl,
    transforms: transform::TransformControl,
    outputs: output::OutputControl,
}

/// An error that can occur when performing a control operation.
#[derive(Debug, Error)]
pub enum ControlError {
    #[error("Cannot send the message because the channel is full")]
    ChannelFull,
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

/// An error that can occur when sending a control message.
///
/// Unlike `ControlError`, `ControlSendError` gives back the message if the channel is full.
#[derive(Debug, Error)]
pub enum ControlSendError {
    #[error("Cannot send the message because the channel is full - {0:?}")]
    ChannelFull(ControlMessage),
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

impl From<ControlSendError> for ControlError {
    fn from(value: ControlSendError) -> Self {
        match value {
            ControlSendError::ChannelFull(_) => ControlError::ChannelFull,
            ControlSendError::Shutdown => ControlError::Shutdown,
        }
    }
}

impl AnonymousControlHandle {
    /// Sends a control message to the pipeline, waiting until there is capacity.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline has been shut down.
    pub async fn send(&self, message: ControlMessage) -> Result<(), ControlSendError> {
        self.tx.send(message).await.map_err(|_| ControlSendError::Shutdown)
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
            inner: self,
            buffer: Vec::new(),
        }
    }

    /// Creates a new buffer for bulk source creation, with the given initial capacity.
    pub fn source_buffer_with_capacity<'a>(&'a mut self, capacity: usize) -> SourceCreationBuffer<'a> {
        SourceCreationBuffer {
            inner: self,
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
        let build = self.managed_source_builder(name, trigger, source);
        self.add_source_builder(build)
    }

    /// Adds a measurement source to the Alumet pipeline, with an explicit builder.
    ///
    /// This is similar to [`AlumetPluginStart::add_source_builder()`](crate::plugin::AlumetPluginStart::add_source_builder()),
    /// except that the builder needs to be [`Send`].
    ///
    /// # Bulk registration of sources
    /// To add multiple sources, it is more efficient to use [`source_buffer`](Self::source_buffer) instead of many `add_source_builder`.
    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&self, builder: F) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::ControlMessage::CreateOne(source::CreateOneMessage {
            plugin: self.plugin.clone(),
            builder: SendSourceBuilder::Managed(Box::new(builder)),
        }));
        self.inner.try_send(message).map_err(|e| e.into())
    }

    /// Adds an autonomous measurement source to the Alumet pipeline, with an explicit builder.
    ///
    /// This is similar to [`AlumetPluginStart::add_autonomous_source_builder()`](crate::plugin::AlumetPluginStart::add_autonomous_source_builder()),
    /// except that the builder needs to be [`Send`].
    ///
    /// # Bulk registration of sources
    /// To add multiple sources, it is more efficient to use [`source_buffer`](Self::source_buffer) instead of many `add_source_builder`.
    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + Send + 'static>(
        &self,
        builder: F,
    ) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::ControlMessage::CreateOne(source::CreateOneMessage {
            plugin: self.plugin.clone(),
            builder: SendSourceBuilder::Autonomous(Box::new(builder)),
        }));
        self.inner.try_send(message).map_err(|e| e.into())
    }

    /// Returns a source builder that returns the given boxed source.
    fn managed_source_builder(
        &self,
        name: &str,
        trigger: trigger::TriggerSpec,
        source: Box<dyn Source>,
    ) -> impl FnOnce(&mut dyn builder::context::SourceBuildContext) -> anyhow::Result<ManagedSourceRegistration> {
        let source_name = name.to_owned();
        move |ctx: &mut dyn builder::context::SourceBuildContext| {
            Ok(ManagedSourceRegistration {
                name: ctx.source_name(&source_name),
                trigger_spec: trigger,
                source,
            })
        }
    }
}

impl SourceCreationBuffer<'_> {
    /// Flushes the buffer: creates all the sources.
    ///
    /// `flush` sends a [`CreateMany`](source::ControlMessage::CreateMany) message that, when processed,
    ///  will create all the sources in this buffer.
    pub fn flush(&mut self) -> Result<(), ControlError> {
        self.inner
            .inner
            .try_send(ControlMessage::Source(source::ControlMessage::CreateMany(
                CreateManyMessage {
                    plugin: self.inner.plugin.clone(),
                    builders: std::mem::take(&mut self.buffer),
                },
            )))
            .map_err(|e| e.into())
    }

    /// Adds a managed source to the buffer.
    pub fn add_source(&mut self, name: &str, source: Box<dyn Source>, trigger: trigger::TriggerSpec) {
        let build = self.inner.managed_source_builder(name, trigger, source);
        self.add_source_builder(build)
    }

    /// Adds a source to the buffer, with an explicit builder.
    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&mut self, builder: F) {
        let builder = SendSourceBuilder::Managed(Box::new(builder));
        self.buffer.push(builder);
    }

    /// Adds an autonomous source builder to the buffer.
    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + Send + 'static>(&mut self, builder: F) {
        let builder = SendSourceBuilder::Autonomous(Box::new(builder));
        self.buffer.push(builder);
    }
}

impl Drop for SourceCreationBuffer<'_> {
    fn drop(&mut self) {
        // prevent forgotten flushes
        if !self.buffer.is_empty() {
            let _ = self.flush(); // ignore errors
        }
    }
}

impl PipelineControl {
    pub fn new(
        sources: source::SourceControl,
        transforms: transform::TransformControl,
        outputs: output::OutputControl,
    ) -> Self {
        Self {
            sources,
            transforms,
            outputs,
        }
    }

    pub fn start(self, shutdown: CancellationToken, on: &runtime::Handle) -> (AnonymousControlHandle, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(256);
        let task = self.run(shutdown.clone(), rx);
        let control_handle = AnonymousControlHandle { tx, shutdown };
        let task_handle = on.spawn(task);
        (control_handle, task_handle)
    }

    async fn handle_message(&mut self, msg: ControlMessage) -> anyhow::Result<()> {
        match msg {
            ControlMessage::Source(msg) => self.sources.handle_message(msg).await,
            ControlMessage::Transform(msg) => self.transforms.handle_message(msg),
            ControlMessage::Output(msg) => self.outputs.handle_message(msg),
        }
    }

    async fn run(mut self, shutdown: CancellationToken, mut rx: Receiver<ControlMessage>) {
        fn task_finished(res: Result<anyhow::Result<()>, tokio::task::JoinError>, kind: &str) {
            match res {
                Ok(Ok(())) => log::debug!("One {kind} task finished without error."),
                Ok(Err(e_normal)) => log::warn!("One {kind} task finished with error: {e_normal}"),
                Err(e_panic) => log::error!("One {kind} task panicked with error: {e_panic:?}"),
            }
        }

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    // The main way to shutdown the pipeline is to cancel the `shutdown` token.
                    // Stop the control loop and shut every element down.
                    break;
                },
                _ = tokio::signal::ctrl_c() => {
                    // Another way to shutdown the pipeline is to send SIGTERM, usually with Ctrl+C.
                    // Tokio's ctrl_c() also handles Ctrl+C on Windows.
                    log::info!("Ctrl+C received, shutting down...");

                    // The token can have child tokens, therefore we need to cancel it instead of simply breaking.
                    shutdown.cancel();
                },
                message = rx.recv() => {
                    // A control message has been received, or the channel has been closed (should not happen).
                    match message {
                        Some(msg) => {
                            if let Err(e) = self.handle_message(msg).await {
                                log::error!("error in message handling: {e:?}");
                            }
                        },
                        None => todo!("pipeline_control_loop#rx channel closed"),
                    }
                },

                // Below we asynchronously poll the source, transform and output tasks, in order to detect
                // when one of them finishes before the entire pipeline is shut down.
                //
                // NOTE: it is important to call `join_next_task()` only if `has_task()`.
                source_res = self.sources.join_next_task(), if self.sources.has_task() => {
                    task_finished(source_res, "source");
                },
                transf_res = self.transforms.join_next_task(), if self.transforms.has_task() => {
                    task_finished(transf_res, "transform");
                }
                output_res = self.outputs.join_next_task(), if self.outputs.has_task() => {
                    task_finished(output_res, "output");
                }
            }
        }
        log::debug!("Pipeline control task shutting down...");

        // Stop the elements, waiting for each step of the pipeline to finish before stopping the next one.
        log::trace!("waiting for sources to finish");
        self.sources.shutdown(|res| task_finished(res, "source")).await;

        log::trace!("waiting for transforms to finish");
        self.transforms.shutdown(|res| task_finished(res, "transform")).await;

        log::trace!("waiting for outputs to finish");
        self.outputs.shutdown(|res| task_finished(res, "output")).await;
    }
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
