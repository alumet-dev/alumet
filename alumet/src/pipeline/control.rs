use super::builder::elements::{
    AutonomousSourceBuilder, ManagedSourceBuilder, ManagedSourceRegistration, SendSourceBuilder,
};
use super::elements::{output, source, transform};
use super::{builder, trigger, PluginName, Source};
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

pub enum ControlError {
    ChannelFull(ControlMessage),
    Shutdown,
}

impl AnonymousControlHandle {
    pub async fn send(&self, message: ControlMessage) -> Result<(), ControlError> {
        self.tx.send(message).await.map_err(|_| ControlError::Shutdown)
    }

    pub fn try_send(&self, message: ControlMessage) -> Result<(), ControlError> {
        match self.tx.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(m)) => Err(ControlError::ChannelFull(m)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ControlError::Shutdown),
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
    pub fn change_plugin_scope(&self, plugin: PluginName) -> ScopedControlHandle {
        ScopedControlHandle {
            inner: self.inner.clone(),
            plugin,
        }
    }

    pub fn anonymous(&self) -> &AnonymousControlHandle {
        &self.inner
    }

    pub fn add_source(
        &self,
        name: &str,
        source: Box<dyn Source>,
        trigger: trigger::TriggerSpec,
    ) -> Result<(), ControlError> {
        let source_name = name.to_owned();
        let build = move |ctx: &mut dyn builder::context::SourceBuildContext| ManagedSourceRegistration {
            name: ctx.source_name(&source_name),
            trigger_spec: trigger,
            source,
        };
        self.add_source_builder(build)
    }

    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&self, builder: F) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::ControlMessage::Create(source::CreateMessage {
            plugin: self.plugin.clone(),
            builder: SendSourceBuilder::Managed(Box::new(builder)),
        }));
        self.inner.try_send(message)
    }

    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + Send + 'static>(
        &self,
        builder: F,
    ) -> Result<(), ControlError> {
        let message = ControlMessage::Source(source::ControlMessage::Create(source::CreateMessage {
            plugin: self.plugin.clone(),
            builder: SendSourceBuilder::Autonomous(Box::new(builder)),
        }));
        self.inner.try_send(message)
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

    fn handle_message(&mut self, msg: ControlMessage) {
        match msg {
            ControlMessage::Source(msg) => self.sources.handle_message(msg),
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
                        Some(msg) => self.handle_message(msg),
                        None => todo!("pipeline_control_loop#rx channel closed"),
                    }
                },

                // Below we asynchronously poll the source, transform and output tasks, in order to detect
                // when one of them finishes before the entire pipeline is shut down.
                source_res = self.sources.join_next_task() => {
                    task_finished(source_res, "source");
                },
                transf_res = self.transforms.join_next_task() => {
                    task_finished(transf_res, "transform");
                }
                output_res = self.outputs.join_next_task() => {
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
