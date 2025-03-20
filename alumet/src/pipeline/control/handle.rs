use std::time::Duration;

use thiserror::Error;
use tokio::{
    sync::mpsc::error::{SendTimeoutError, TrySendError},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::pipeline::{error::PipelineError, naming::PluginName};

use super::{messages, request};

/// A control handle that is not tied to a particular plugin.
///
/// Unlike [`ScopedControlHandle`], `AnonymousControlHandle` does not provide any method
/// that register new pipeline elements. You can call [`AnonymousControlHandle::scoped`] to turn an anonymous handle
/// into a scoped one.
#[derive(Clone)]
pub struct AnonymousControlHandle {
    pub(super) tx: messages::Sender,
    pub(super) shutdown_token: CancellationToken,
}

#[derive(Clone)]
pub struct PluginControlHandle {
    pub(super) inner: AnonymousControlHandle,
    pub(super) plugin: PluginName,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DispatchError {
    /// The pipeline controller was not available.
    /// This happens when the pipeline is shut down before dispatching the request.
    #[error("dispatch failed: pipeline controller not available")]
    NotAvailable,
    /// The deadline has expired.
    #[error("dispatch failed: timeout expired")]
    Timeout,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SendWaitError {
    /// The pipeline controlled was not available.
    /// This happens when the pipeline is shut down before processing the request.
    #[error("send_wait failed: pipeline controller not available")]
    NotAvailable,
    /// The deadline has expired.
    #[error("send_wait failed: timeout expired")]
    Timeout,
    /// The request was processed by the pipeline controller, but it returned an error.
    ///
    /// This does not always mean that the entire operation failed.
    /// It could be a partial failure. For instance, if your requested the creation of
    /// multiple elements, some of them may have been created successfully while others
    /// have failed.
    #[error("send_wait failed: processing the request returned an error")]
    Operation(#[source] PipelineError),
}

pub enum OnBackgroundError {
    Log,
}

impl AnonymousControlHandle {
    pub fn with_plugin(self, plugin: PluginName) -> PluginControlHandle {
        PluginControlHandle { inner: self, plugin }
    }

    /// Shuts the pipeline down.
    pub fn shutdown(&self) {
        self.shutdown_token.cancel();
    }

    /// Sends a control request to the pipeline, without waiting for a response.
    ///
    /// # Errors
    /// If the pipeline has been shut down, returns a `NotAvailable` error.
    #[allow(private_bounds)] // intended: only us should be able to implement request traits
    pub async fn dispatch(
        &self,
        request: impl request::AnonymousControlRequest,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<(), DispatchError> {
        self.impl_dispatch(request.serialize(), timeout.into()).await
    }

    /// Sends a control request to the pipeline, and waits for a response.
    ///
    /// Unlike [`dispatch`], `send_wait` waits for the request to be processed
    /// by the pipeline and returns its result.
    ///
    /// # Errors
    /// If the pipeline is shut down before the request is processed, the function
    /// returns a `NotAvailable` error.
    #[allow(private_bounds)]
    pub async fn send_wait<R>(
        &self,
        request: impl request::AnonymousControlRequest<OkResponse = R>,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<R, SendWaitError> {
        let (msg, rx) = request.serialize_with_response();
        self.impl_send_wait(msg, rx, timeout.into()).await
    }

    /// Sends a request without waiting for a response and without blocking.
    ///
    /// If the request cannot be sent immediately, spawns a background task on the current Tokio runtime.
    /// Background task failures are handled according to the `on_error` strategy.
    ///
    /// # Errors
    /// If the pipeline has been shut down, returns a `NotAvailable` error.
    ///
    /// # Panics
    /// Panics if not called in the context of a Tokio runtime.
    #[allow(private_bounds)]
    pub fn dispatch_in_current_runtime(
        &self,
        request: impl request::AnonymousControlRequest,
        timeout: impl Into<Option<Duration>>,
        on_error: OnBackgroundError,
    ) -> Result<(), DispatchError> {
        let _ = on_error; // there is only one strategy, it's hardcoded in impl
        self.impl_dispatch_in_current_runtime(request.serialize(), timeout.into())
            .map(|_| ())
    }

    async fn impl_dispatch(
        &self,
        msg: messages::ControlRequest,
        timeout: Option<Duration>,
    ) -> Result<(), DispatchError> {
        match timeout {
            Some(timeout) => self.tx.send_timeout(msg, timeout).await.map_err(|e| match e {
                SendTimeoutError::Timeout(_) => DispatchError::Timeout,
                SendTimeoutError::Closed(_) => DispatchError::NotAvailable,
            }),
            None => self.tx.send(msg).await.map_err(|_| DispatchError::NotAvailable),
        }
    }

    async fn impl_send_wait<R>(
        &self,
        msg: messages::ControlRequest,
        rx: impl request::ResponseReceiver<Ok = R>,
        timeout: Option<Duration>,
    ) -> Result<R, SendWaitError> {
        // send the message
        match timeout {
            Some(timeout) => self.tx.send_timeout(msg, timeout).await.map_err(|e| match e {
                SendTimeoutError::Timeout(_) => SendWaitError::Timeout,
                SendTimeoutError::Closed(_) => SendWaitError::NotAvailable,
            }),
            None => self.tx.send(msg).await.map_err(|_| SendWaitError::NotAvailable),
        }?;
        // wait for a response
        match rx.recv().await {
            Ok(result) => match result {
                Ok(ret) => Ok(ret),
                Err(err) => Err(SendWaitError::Operation(err)),
            },
            Err(_recv_error) => Err(SendWaitError::NotAvailable),
        }
    }

    fn impl_dispatch_in_current_runtime(
        &self,
        msg: messages::ControlRequest,
        timeout: Option<Duration>,
    ) -> Result<Option<JoinHandle<Result<(), DispatchError>>>, DispatchError> {
        // get the handle to the current runtime
        let current = tokio::runtime::Handle::try_current()
            .expect("dispatch_in_current_runtime must be called within a Tokio runtime. If you are not in a thread that is managed by Alumet, a potential solution is to create a runtime yourself.");

        // attempt to send the message right now
        match self.tx.try_send(msg) {
            Ok(()) => Ok(None),
            Err(TrySendError::Closed(_)) => Err(DispatchError::NotAvailable),
            Err(TrySendError::Full(msg)) => {
                // the message queue is full, we need to wait in an async task
                let control_handle = self.clone();
                let task_handle = current.spawn(async move {
                    let res = control_handle.impl_dispatch(msg, timeout).await;
                    if let Err(e) = &res {
                        log::error!("dispacth failed in background: {e:?}");
                    }
                    res
                });
                Ok(Some(task_handle))
            }
        }
    }
}

impl PluginControlHandle {
    pub fn anonymous(self) -> AnonymousControlHandle {
        self.inner
    }

    /// Sends a control request to the pipeline, without waiting for a response.
    ///
    /// # Errors
    /// If the pipeline has been shut down, returns a `NotAvailable` error.
    #[allow(private_bounds)]
    pub async fn dispatch(
        &self,
        request: impl request::PluginControlRequest,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<(), DispatchError> {
        let body = request.serialize(&self.plugin);
        self.inner.impl_dispatch(body, timeout.into()).await
    }

    /// Sends a control request to the pipeline, and waits for a response.
    ///
    /// Unlike [`dispatch`], `send_wait` waits for the request to be processed
    /// by the pipeline and returns its result.
    ///
    /// # Errors
    /// If the pipeline is shut down before the request is processed, the function
    /// returns a `NotAvailable` error.
    #[allow(private_bounds)]
    pub async fn send_wait<R>(
        &self,
        request: impl request::PluginControlRequest<OkResponse = R>,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<R, SendWaitError> {
        let (msg, rx) = request.serialize_with_response(&self.plugin);
        self.inner.impl_send_wait(msg, rx, timeout.into()).await
    }

    /// Shuts the pipeline down.
    pub fn shutdown(&self) {
        self.inner.shutdown();
    }

    /// Sends a request without waiting for a response and without blocking.
    ///
    /// If the request cannot be sent immediately, spawns a background task on the current Tokio runtime.
    /// Background task failures are handled according to the `on_error` strategy.
    ///
    /// # Errors
    /// If the pipeline has been shut down, returns a `NotAvailable` error.
    ///
    /// # Panics
    /// Panics if not called in the context of a Tokio runtime.
    #[allow(private_bounds)]
    pub fn dispatch_in_current_runtime(
        &self,
        request: impl request::PluginControlRequest,
        timeout: impl Into<Option<Duration>>,
        on_error: OnBackgroundError,
    ) -> Result<(), DispatchError> {
        let _ = on_error;
        let request = request.serialize(&self.plugin);
        self.inner
            .impl_dispatch_in_current_runtime(request, timeout.into())
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use crate::pipeline::util::assert_send;

    use super::{AnonymousControlHandle, PluginControlHandle};

    #[test]
    fn types() {
        assert_send::<AnonymousControlHandle>();
        assert_send::<PluginControlHandle>();
    }
}
