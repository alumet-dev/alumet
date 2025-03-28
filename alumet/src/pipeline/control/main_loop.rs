//! On-the-fly modification of the pipeline.
use crate::pipeline::control::messages::RequestMessage;
use crate::pipeline::error::PipelineError;

use crate::pipeline::elements::{output, source, transform};

use anyhow::anyhow;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{messages, AnonymousControlHandle};

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
        let control_handle = AnonymousControlHandle {
            tx,
            shutdown_token: shutdown,
        };
        let task_handle = on.spawn(task);
        (control_handle, task_handle)
    }

    async fn handle_message(&mut self, msg: messages::ControlRequest) -> Result<(), PipelineError> {
        /// Responds to a message with a value of type `Result<R, PipelineError>`.
        fn send_response<R>(
            result: Result<R, PipelineError>,
            response_tx: Option<messages::ResponseSender<R>>,
        ) -> Result<(), PipelineError> {
            match response_tx {
                Some(tx) => tx
                    .send(result)
                    .map_err(|_| PipelineError::internal(anyhow!("failed to send control response"))),
                None => {
                    // those who has sent the message does not care about the response, discard it
                    result.map(|_| ())
                }
            }
        }

        // ControlRequest uses variants for each response type.
        match msg {
            messages::ControlRequest::NoResult(RequestMessage { response_tx, body }) => {
                let result = match body {
                    messages::EmptyResponseBody::Source(msg) => self.sources.handle_message(msg).await,
                    messages::EmptyResponseBody::Transform(msg) => self.transforms.handle_message(msg),
                    messages::EmptyResponseBody::Output(msg) => self.outputs.handle_message(msg),
                };
                send_response(result.map_err(PipelineError::internal), response_tx)
            }
            messages::ControlRequest::Introspect(RequestMessage { response_tx, body }) => {
                let result = match body {
                    messages::IntrospectionBody::ListElements(filter) => {
                        let mut buf = Vec::new();
                        self.sources.list_elements(&mut buf, &filter);
                        self.transforms.list_elements(&mut buf, &filter);
                        self.outputs.list_elements(&mut buf, &filter);
                        Ok(buf)
                    }
                };
                send_response(result, response_tx)
            }
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
        mut rx: messages::Receiver,
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
        // It is particularily useful in tests, to assert that no error occurred.
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
                            log::trace!("handling {msg:?}");
                            if let Err(e) = self.handle_message(msg).await {
                                log::error!("error in message handling: {e:?}");
                                last_error = Err(e);
                            }
                        },
                        None => todo!("pipeline_control_loop#rx channel closed"),
                    }
                },

                // Below we asynchronously poll the source, transform and output tasks, in order to detect
                // when one of them finishes before the entire pipeline is shut down.
                //
                // IMPORTANT: if a JoinSet is empty, `join_next_task` will immediately return
                // `Poll::Ready(None)` when polled, which will cause an infinite loop.
                //
                // The solution is to NOT poll `join_next_task` if there is no task in the set.
                // The condition `has_task` reads a single boolean variable, hence it's very cheap.
                //
                // Since the only way to add new tasks to the JoinSet is to send a control message,
                // and this is handled by a separate branch, we are good.
                // NOTE: if the above paragraph becomes untrue, another solution needs to be found.
                //
                // Example scenario:
                // - loop, sources JoinSet empty => branch disabled, we only poll cancelled(), ctrl_c() and rx.recv()
                // - we receive a message, add a source to the JoinSet
                // - loop, sources JoinSet not empty => branch enabled

                res = self.sources.join_next_task(), if self.sources.has_task() => {
                    task_finished(res, "source", &mut last_error);
                },
                res = self.transforms.join_next_task(), if self.transforms.has_task() => {
                    task_finished(res, "transform", &mut last_error);
                }
                res = self.outputs.join_next_task(), if self.outputs.has_task() => {
                    task_finished(res, "output", &mut last_error);
                }
            }
        }
        log::debug!("Pipeline control task shutting down...");

        // Stop the elements, waiting for each step of the pipeline to finish before stopping the next one.
        log::trace!("waiting for sources to finish");
        self.sources
            .shutdown(|res| task_finished(res, "source", &mut last_error))
            .await;

        log::trace!("waiting for transforms to finish");
        self.transforms
            .shutdown(|res| task_finished(res, "transform", &mut last_error))
            .await;

        log::trace!("waiting for outputs to finish");
        self.outputs
            .shutdown(|res| task_finished(res, "output", &mut last_error))
            .await;

        // Finalize the shutdown sequence by cancelling the remaining things.
        finalize_shutdown.cancel();
        last_error.map_err(|e| PipelineError::from(e))
    }
}
