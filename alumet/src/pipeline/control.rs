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

#[derive(Clone)]
pub struct AnonymousControlHandle {
    tx: Sender<ControlMessage>,
    shutdown: CancellationToken,
}

#[derive(Clone)]
pub struct ScopedControlHandle {
    inner: AnonymousControlHandle,
    plugin: PluginName,
}

pub struct SourceCreationBuffer<'a> {
    inner: &'a mut ScopedControlHandle,
    buffer: Vec<builder::elements::SendSourceBuilder>,
}

#[derive(Debug)]
pub enum ControlMessage {
    Source(source::ControlMessage),
    Transform(transform::ControlMessage),
    Output(output::ControlMessage),
}

pub(crate) struct PipelineControl {
    sources: source::SourceControl,
    transforms: transform::TransformControl,
    outputs: output::OutputControl,
}

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("Cannot send the message because the channel is full")]
    ChannelFull,
    #[error("Cannot send the message because the pipeline has shut down")]
    Shutdown,
}

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
    pub async fn send(&self, message: ControlMessage) -> Result<(), ControlSendError> {
        self.tx.send(message).await.map_err(|_| ControlSendError::Shutdown)
    }

    pub fn try_send(&self, message: ControlMessage) -> Result<(), ControlSendError> {
        match self.tx.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(m)) => Err(ControlSendError::ChannelFull(m)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ControlSendError::Shutdown),
        }
    }

    pub fn shutdown(&self) {
        self.shutdown.cancel()
    }

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

    pub fn source_buffer<'a>(&'a mut self) -> SourceCreationBuffer<'a> {
        SourceCreationBuffer {
            inner: self,
            buffer: Vec::new(),
        }
    }

    pub fn source_buffer_with_capacity<'a>(&'a mut self, capacity: usize) -> SourceCreationBuffer<'a> {
        SourceCreationBuffer {
            inner: self,
            buffer: Vec::with_capacity(capacity),
        }
    }

    pub fn add_source(
        &self,
        name: &str,
        source: Box<dyn Source>,
        trigger: trigger::TriggerSpec,
    ) -> Result<(), ControlError> {
        let build = self.managed_source_builder(name, trigger, source);
        self.add_source_builder(build)
    }

    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&self, builder: F) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::ControlMessage::CreateOne(source::CreateOneMessage {
            plugin: self.plugin.clone(),
            builder: SendSourceBuilder::Managed(Box::new(builder)),
        }));
        self.inner.try_send(message).map_err(|e| e.into())
    }

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

    pub fn add_source(&mut self, name: &str, source: Box<dyn Source>, trigger: trigger::TriggerSpec) {
        let build = self.inner.managed_source_builder(name, trigger, source);
        self.add_source_builder(build)
    }

    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&mut self, builder: F) {
        let builder = SendSourceBuilder::Managed(Box::new(builder));
        self.buffer.push(builder);
    }

    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + Send + 'static>(&mut self, builder: F) {
        let builder = SendSourceBuilder::Autonomous(Box::new(builder));
        self.buffer.push(builder);
    }
}

impl Drop for SourceCreationBuffer<'_> {
    fn drop(&mut self) {
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
