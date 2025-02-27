//! On-the-fly modification of the pipeline.
use crate::pipeline::error::PipelineError;

use super::elements::{output, source, transform};

use tokio::runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub mod error;
pub mod handle;
pub mod key;
pub mod message;
mod source_buffer;

pub use handle::{AnonymousControlHandle, ScopedControlHandle};
pub use message::ControlMessage;
pub use source_buffer::SourceCreationBuffer;

/// Encapsulates sources, transforms and outputs control.
pub(crate) struct PipelineControl {
    sources: source::control::SourceControl,
    transforms: transform::control::TransformControl,
    outputs: output::control::OutputControl,
}

impl PipelineControl {
    pub fn new(
        sources: source::control::SourceControl,
        transforms: transform::control::TransformControl,
        outputs: output::control::OutputControl,
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
    ) -> (AnonymousControlHandle, JoinHandle<Result<(), PipelineError>>) {
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

    /// Main control loop of the measurement pipeline.
    ///
    /// The role of this function is to "oversee" the operation of the pipeline by:
    /// - checking if the pipeline should be shut down
    /// - receiving control messages and forwarding them to the appropriate handling code
    /// - polling the async tasks to cleanup the tasks that have finished
    ///
    /// When the pipeline is requested to shut down, `run` exits from the control loop and
    /// waits for the elements to finish. This can take an arbitrarily long time to complete
    /// (e.g. because of a bug in an element), therefore `run` should be wrapped in
    /// [`tokio::time::timeout`];
    async fn run(
        mut self,
        init_shutdown: CancellationToken,
        finalize_shutdown: CancellationToken,
        mut rx: mpsc::Receiver<ControlMessage>,
    ) -> Result<(), PipelineError> {
        fn task_finished(
            res: Result<Result<(), PipelineError>, tokio::task::JoinError>,
            kind: &'static str,
            result: &mut Result<(), PipelineError>,
        ) {
            match res {
                Ok(Ok(())) => log::debug!("One {kind} task finished without error."),
                Ok(Err(e_normal)) => {
                    log::error!("One {kind} task finished with error: {e_normal}");
                    *result = Err(e_normal);
                }
                Err(e) if e.is_cancelled() => {
                    log::error!("{kind} cancelled: {e:?}");
                    *result = Err(PipelineError::internal(e.into()));
                }
                Err(e_panic) => {
                    log::error!("One {kind} task panicked with error: {e_panic:?}");
                    *result = Err(PipelineError::internal(e_panic.into()));
                }
            }
        }

        // Keep track of the most recent error, so we can propagate it to the agent.
        // It is particularily useful in tests, to assert that no error occured.
        let mut last_error: Result<(), PipelineError> = Result::Ok(());

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
                                last_error = Err(PipelineError::internal(e));
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
                    task_finished(source_res, "source", &mut last_error);
                },
                transf_res = self.transforms.join_next_task(), if self.transforms.has_task() => {
                    task_finished(transf_res, "transform", &mut last_error);
                }
                output_res = self.outputs.join_next_task(), if self.outputs.has_task() => {
                    task_finished(output_res, "output", &mut last_error);
                }
            }
        }
        log::debug!("Pipeline control task shutting down...");

        // Stop the elements, waiting for each step of the pipeline to finish before stopping the next one.
        log::trace!("waiting for sources to finish");
        self.sources.shutdown(|res| task_finished(res, "source", &mut last_error)).await;

        log::trace!("waiting for transforms to finish");
        self.transforms.shutdown(|res| task_finished(res, "transform", &mut last_error)).await;

        log::trace!("waiting for outputs to finish");
        self.outputs.shutdown(|res| task_finished(res, "output", &mut last_error)).await;

        // Finalize the shutdown sequence by cancelling the remaining things.
        finalize_shutdown.cancel();
        last_error.map_err(|e| PipelineError::from(e))
    }
}
