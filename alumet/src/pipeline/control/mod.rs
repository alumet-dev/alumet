//! On-the-fly modification of the pipeline.
use super::elements::{output, source, transform};

use tokio::runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub mod error;
pub mod handle;
mod source_buffer;

pub use handle::{AnonymousControlHandle, ControlMessage, ScopedControlHandle};
pub use source_buffer::SourceCreationBuffer;

/// Encapsulates sources, transforms and outputs control.
pub(crate) struct PipelineControl {
    sources: source::SourceControl,
    transforms: transform::TransformControl,
    outputs: output::OutputControl,
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

    pub fn start(
        self,
        shutdown: CancellationToken,
        finalize_shutdown: CancellationToken,
        on: &runtime::Handle,
    ) -> (AnonymousControlHandle, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(256);
        let task = self.run(shutdown.clone(), finalize_shutdown, rx);
        let control_handle = AnonymousControlHandle::new(tx, shutdown);
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

    async fn run(
        mut self,
        init_shutdown: CancellationToken,
        finalize_shutdown: CancellationToken,
        mut rx: mpsc::Receiver<ControlMessage>,
    ) {
        fn task_finished(res: Result<anyhow::Result<()>, tokio::task::JoinError>, kind: &str) {
            match res {
                Ok(Ok(())) => log::debug!("One {kind} task finished without error."),
                Ok(Err(e_normal)) => log::warn!("One {kind} task finished with error: {e_normal}"),
                Err(e_panic) => log::error!("One {kind} task panicked with error: {e_panic:?}"),
            }
        }

        loop {
            tokio::select! {
                _ = init_shutdown.cancelled() => {
                    // The main way to shutdown the pipeline is to cancel the `shutdown` token.
                    // Stop the control loop and shut every element down.
                    break;
                },
                _ = tokio::signal::ctrl_c() => {
                    // Another way to shutdown the pipeline is to send SIGTERM, usually with Ctrl+C.
                    // Tokio's ctrl_c() also handles Ctrl+C on Windows.
                    log::info!("Ctrl+C received, shutting down...");

                    // The token can have child tokens, therefore we need to cancel it instead of simply breaking.
                    init_shutdown.cancel();
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

        // Finalize the shutdown sequence by cancelling the remaining things.
        finalize_shutdown.cancel();
    }
}
