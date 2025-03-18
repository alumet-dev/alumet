use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc::{error::SendTimeoutError, Sender};
use tokio_util::sync::CancellationToken;

use crate::pipeline::{error::PipelineError, naming::PluginName};

use super::{
    main_loop::{ControlRequestBody, ControlRequestMessage},
    request,
};

/// A control handle that is not tied to a particular plugin.
///
/// Unlike [`ScopedControlHandle`], `AnonymousControlHandle` does not provide any method
/// that register new pipeline elements. You can call [`AnonymousControlHandle::scoped`] to turn an anonymous handle
/// into a scoped one.
#[derive(Clone)]
pub struct AnonymousControlHandle {
    pub(super) tx: Sender<ControlRequestMessage>,
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
    pub async fn dispatch(
        &self,
        request: impl request::ControlRequest,
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
    pub async fn send_wait(
        &self,
        request: impl request::ControlRequest,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<(), SendWaitError> {
        self.impl_send_wait(request.serialize(), timeout.into()).await
    }

    async fn impl_dispatch(&self, body: ControlRequestBody, timeout: Option<Duration>) -> Result<(), DispatchError> {
        let msg = ControlRequestMessage { response: None, body };
        match timeout {
            Some(timeout) => self.tx.send_timeout(msg, timeout).await.map_err(|e| match e {
                SendTimeoutError::Timeout(_) => DispatchError::Timeout,
                SendTimeoutError::Closed(_) => DispatchError::NotAvailable,
            }),
            None => self.tx.send(msg).await.map_err(|_| DispatchError::NotAvailable),
        }
    }

    async fn impl_send_wait(&self, body: ControlRequestBody, timeout: Option<Duration>) -> Result<(), SendWaitError> {
        // open a channel to allow the message handler to send us a response
        let (tx, rx) = tokio::sync::oneshot::channel();
        let msg = ControlRequestMessage {
            response: Some(tx),
            body,
        };
        // send the message
        match timeout {
            Some(timeout) => self.tx.send_timeout(msg, timeout).await.map_err(|e| match e {
                SendTimeoutError::Timeout(_) => SendWaitError::Timeout,
                SendTimeoutError::Closed(_) => SendWaitError::NotAvailable,
            }),
            None => self.tx.send(msg).await.map_err(|_| SendWaitError::NotAvailable),
        }?;
        // wait for a response
        match rx.await {
            Ok(ret) => match ret.result {
                Ok(_) => Ok(()),
                Err(err) => Err(SendWaitError::Operation(err)),
            },
            Err(_recv_error) => Err(SendWaitError::NotAvailable),
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
    pub async fn send_wait(
        &self,
        request: impl request::PluginControlRequest,
        timeout: impl Into<Option<Duration>>,
    ) -> Result<(), SendWaitError> {
        let body = request.serialize(&self.plugin);
        self.inner.impl_send_wait(body, timeout.into()).await
    }

    /// Shuts the pipeline down.
    pub fn shutdown(&self) {
        self.inner.shutdown();
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
