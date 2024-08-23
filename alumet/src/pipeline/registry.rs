//! Registry of metrics common to the whole pipeline.

use std::sync::Arc;

use tokio::{
    runtime,
    sync::{
        mpsc::{self, Receiver},
        oneshot, RwLock, RwLockReadGuard,
    },
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::metrics::{Metric, MetricCreationError, MetricRegistry, RawMetricId};

/// A message that can be sent to the task that controls the [`MetricRegistry`],
/// for instance via [`MetricSender`].
pub enum ControlMessage {
    /// Registers new metrics.
    RegisterMetrics(
        Vec<Metric>,
        DuplicateStrategy,
        Option<oneshot::Sender<Vec<Result<RawMetricId, MetricCreationError>>>>,
    ),
    /// Adds a new listener that will be notified on new metric registration.
    Subscribe(MetricListener),
}

/// A strategy to handle duplicate metrics.
#[derive(Debug)]
pub enum DuplicateStrategy {
    /// Return an error immediately.
    Error,
    /// Rename the duplicate metrics by appending a suffix and an integer to its name.
    Rename { suffix: String },
}

/// A callback that gets notified of new metrics.
pub type MetricListener = Box<dyn Fn(Vec<(RawMetricId, Metric)>) + Send>;

pub(crate) struct MetricRegistryControl {
    registry: Arc<RwLock<MetricRegistry>>,
    listeners: Vec<MetricListener>,
}

impl MetricRegistryControl {
    pub fn new(registry: MetricRegistry, listeners: Vec<MetricListener>) -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry)),
            listeners,
        }
    }

    pub fn start(
        self,
        shutdown: CancellationToken,
        on: &runtime::Handle,
    ) -> (MetricSender, MetricAccess, JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(256);
        let reader = MetricAccess {
            inner: self.registry.clone(),
        };
        let sender = MetricSender(tx);
        let task = self.run(shutdown.clone(), rx);
        let task_handle = on.spawn(task);
        (sender, reader, task_handle)
    }

    async fn handle_message(&mut self, msg: ControlMessage) {
        match msg {
            ControlMessage::RegisterMetrics(metrics, dup, reply_to) => {
                // Use an RCU (Read, Copy, Update) scheme to modify the registry with the minimal
                // amount of blocking for readers and writers.
                //
                // NOTE: Since we don't take the write lock when copying, we must ensure that only one thread
                // is performing the copy and the update (otherwise we would end up with multiple
                // desynchronized copies). This is achieved by handling all the messages in one
                // task, thus making their processing sequential.

                // read and copy
                let mut copy = (*self.registry.read().await).clone();
                // modify the copy
                let res = match dup {
                    DuplicateStrategy::Error => copy.extend(metrics.clone()),
                    DuplicateStrategy::Rename { suffix } => copy
                        .extend_infallible(metrics.clone(), &suffix)
                        .into_iter()
                        .map(|res| Ok(res))
                        .collect(),
                };
                // update
                *self.registry.write().await = copy;

                // call listeners
                let mut registered_metrics = Vec::with_capacity(res.len());
                for (metric, maybe_id) in metrics.into_iter().zip(res.iter()) {
                    if let Ok(id) = maybe_id {
                        registered_metrics.push((*id, metric));
                    }
                }
                match &self.listeners[..] {
                    [] => (),
                    [listener] => listener(registered_metrics),
                    listeners => {
                        for listener in listeners {
                            listener(registered_metrics.clone());
                        }
                    }
                }

                // send reply
                if let Some(tx) = reply_to {
                    if let Err(e) = tx.send(res) {
                        log::error!("Failed to send reply to metric registration message: {e:?}");
                    }
                }
            }
            ControlMessage::Subscribe(listener) => {
                self.listeners.push(listener);
            }
        }
    }

    pub async fn run(mut self, shutdown: CancellationToken, mut rx: Receiver<ControlMessage>) {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    break;
                },
                message = rx.recv() => {
                    match message {
                        Some(msg) => self.handle_message(msg).await,
                        None => todo!("registry_control_loop#rx channel closed")
                    }
                }
            }
        }
    }
}

/// Read-write access to a [`MetricRegistry`] from multiple threads.
#[derive(Clone)]
pub struct MetricAccess {
    inner: Arc<RwLock<MetricRegistry>>,
}

/// Read-only access to a [`MetricRegistry`].
#[derive(Clone)]
pub struct MetricReader(MetricAccess);

/// Indirect write access to a [`MetricRegistry`] and subscription facilities
/// for registry updates.
#[derive(Clone)]
pub struct MetricSender(mpsc::Sender<ControlMessage>);

/// The message could not be sent.
pub enum SendError {
    /// The channel is full.
    ChannelFull(ControlMessage),
    /// The task that controls the registry has been shut down.
    ///
    /// This happens when the Alumet pipeline is shut down.
    Shutdown,
}

/// The message could not be sent, or its reply could not be obtained.
pub enum SendWithReplyError {
    /// The message could not be sent.
    Send(SendError),
    /// The message was sent but it was impossible to get a response from the
    /// task that controls the registry.
    Recv(oneshot::error::RecvError),
}

pub(crate) fn make_listener<F: Fn(Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> + Send + 'static>(
    listener: F,
) -> MetricListener {
    Box::new(move |new_metrics| {
        if let Err(err) = listener(new_metrics) {
            log::error!("Error in metric registration listener: {err:?}");
        };
    })
}

impl MetricAccess {
    /// Provides shared read access to the metric registry.
    pub async fn read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.inner.read().await
    }

    /// Provides exclusive write access to the metric registry.
    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<MetricRegistry> {
        self.inner.write().await
    }

    pub fn into_read_only(self) -> MetricReader {
        MetricReader(self)
    }
}

impl MetricReader {
    /// Provides shared read access to the metric registry.
    pub async fn read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.0.read().await
    }

    /// Provides shared read access to the metric registry, **in a blocking way**.
    ///
    /// Only use this _outside_ of an async runtime.
    pub(crate) fn blocking_read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.0.inner.blocking_read()
    }
}

impl MetricSender {
    /// Sends a message to the metric control loop. Waits until there is capacity.
    /// # Errors
    ///
    /// Returns an error if the pipeline has been shut down.
    pub async fn send(&self, message: ControlMessage) -> Result<(), SendError> {
        self.0.send(message).await.map_err(|_| SendError::Shutdown)
    }

    /// Attempts to immediately send a message to the metric control loop.
    ///
    /// # Errors
    ///
    /// There are two possible cases:
    /// - The pipeline has been shut down and can no longer accept any message.
    /// - The buffer of the control channel is full.
    pub fn try_send(&self, message: ControlMessage) -> Result<(), SendError> {
        match self.0.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(m)) => Err(SendError::ChannelFull(m)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SendError::Shutdown),
        }
    }

    /// Registers new metrics.
    ///
    /// # Errors
    /// The registration of a metric fails if another metric with the same unique name has already been registered.
    /// For each metric, a `Result<RawMetricId, MetricCreationError>` is returned.
    ///
    /// `create_metrics` can also fail if the control message cannot be sent, or if the registration reply cannot be received.
    pub async fn create_metrics(
        &self,
        metrics: Vec<Metric>,
        on_duplicate: DuplicateStrategy,
    ) -> Result<Vec<Result<RawMetricId, MetricCreationError>>, SendWithReplyError> {
        let (tx, rx) = oneshot::channel();
        let message = ControlMessage::RegisterMetrics(metrics, on_duplicate, Some(tx));
        self.send(message).await.map_err(|e| SendWithReplyError::Send(e))?;
        let result = rx.await.map_err(|e| SendWithReplyError::Recv(e))?;
        Ok(result)
    }

    /// Attempts to add a new metric listener immediately.
    pub fn try_subscribe<F: Fn(Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> + Send + 'static>(
        &self,
        listener: F,
    ) -> Result<(), SendError> {
        self.try_send(ControlMessage::Subscribe(make_listener(listener)))
    }

    /// Adds a new metric listener. Waits until there is capacity to send the message.
    pub async fn subscribe<F: Fn(Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> + Send + 'static>(
        &self,
        listener: F,
    ) -> Result<(), SendError> {
        self.send(ControlMessage::Subscribe(make_listener(listener))).await
    }
}
