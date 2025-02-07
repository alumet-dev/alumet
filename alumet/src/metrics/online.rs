//! Online interaction with the metric registry.

use std::{fmt::Debug, sync::Arc};

use anyhow::Context;
use listener::{MetricListenerBuilder, MetricListenerRegistration};
use tokio::{
    runtime,
    sync::{
        mpsc::{self, Receiver},
        oneshot, RwLock, RwLockReadGuard,
    },
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::pipeline::{util::naming::NameGenerator, PluginName};

use super::{
    def::{Metric, RawMetricId},
    error::MetricCreationError,
    registry::MetricRegistry,
};

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
    Subscribe(PluginName, Box<dyn listener::MetricListenerBuilder + Send>),
}

/// A strategy to handle duplicate metrics.
#[derive(Debug)]
pub enum DuplicateStrategy {
    /// Return an error immediately.
    Error,
    /// Rename the duplicate metrics by appending a suffix and an integer to its name.
    Rename { suffix: String },
    // TODO distinguish between "strictly rejecting the metric if one with the same name exists"
    // and "rejecting the metric if one with the same name _and_ a different definiton exists"
}

/// Controls the central registry of metrics.
pub(crate) struct MetricRegistryControl {
    registry: Arc<RwLock<MetricRegistry>>,
    listeners: Vec<listener::MetricListenerRegistration>,
    listener_names: NameGenerator,
}

pub mod listener {
    use crate::{
        metrics::def::{Metric, RawMetricId},
        pipeline::util::naming::{ListenerName, PluginElementNamespace},
    };

    /// A callback that gets notified of new metrics.
    pub trait MetricListener: FnMut(Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> + Send {}
    impl<F> MetricListener for F where F: FnMut(Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> + Send {}

    pub(super) struct BuildContext<'a> {
        pub(super) rt: &'a tokio::runtime::Handle,
        pub(super) namegen: &'a mut PluginElementNamespace,
    }

    /// Context accessible when building a metric listener.
    pub trait MetricListenerBuildContext {
        /// Generates a name for the listener.
        fn listener_name(&mut self, name: &str) -> ListenerName;
        /// Returns a handle to the async runtime on which the listener will be executed.
        fn async_runtime(&self) -> &tokio::runtime::Handle;
    }

    impl MetricListenerBuildContext for BuildContext<'_> {
        fn listener_name(&mut self, name: &str) -> ListenerName {
            ListenerName(self.namegen.insert_deduplicate(name))
        }

        fn async_runtime(&self) -> &tokio::runtime::Handle {
            self.rt
        }
    }

    /// Information required to register a new metric listener.
    pub struct MetricListenerRegistration {
        pub name: ListenerName,
        pub listener: Box<dyn MetricListener>,
    }

    /// Trait for builders of metric listeners.
    ///
    /// Similar to the other builders.
    pub trait MetricListenerBuilder:
        FnOnce(&mut dyn MetricListenerBuildContext) -> anyhow::Result<MetricListenerRegistration>
    {
    }
    impl<F> MetricListenerBuilder for F where
        F: FnOnce(&mut dyn MetricListenerBuildContext) -> anyhow::Result<MetricListenerRegistration>
    {
    }
}

impl MetricRegistryControl {
    /// Creates a new `MetricRegistryControl` and registers some listeners.
    ///
    /// The listeners will be notified of the metric registrations that occur after
    /// [`start`](Self::start) is called. They do _not_ get notified of the metrics
    /// that are initially present in the registry.
    pub fn new(registry: MetricRegistry) -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry)),
            listeners: Vec::new(),
            listener_names: NameGenerator::new(),
        }
    }

    pub fn create_listeners(
        &mut self,
        builders: Vec<(PluginName, Box<dyn MetricListenerBuilder>)>,
        rt: &tokio::runtime::Handle,
    ) -> anyhow::Result<()> {
        self.listeners.reserve_exact(builders.len());
        for (plugin, builder) in builders {
            let mut ctx = listener::BuildContext {
                rt,
                namegen: self.listener_names.plugin_namespace(&plugin),
            };
            let reg = builder(&mut ctx)
                .with_context(|| format!("error in listener creation requested by plugin {plugin}"))?;
            self.listeners.push(reg);
        }
        Ok(())
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
        fn call_listener(reg: &mut MetricListenerRegistration, metrics: Vec<(RawMetricId, Metric)>) {
            let MetricListenerRegistration { name, ref mut listener } = reg;
            let n = metrics.len();
            if let Err(e) = listener(metrics) {
                log::error!("Error in metric listener {name} (called on {n} metrics): {e}",);
            }
        }

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
                match &mut self.listeners[..] {
                    [] => (),
                    [listener] => call_listener(listener, registered_metrics),
                    listeners => {
                        for listener in listeners {
                            call_listener(listener, registered_metrics.clone());
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
            ControlMessage::Subscribe(plugin, listener) => {
                let rt = tokio::runtime::Handle::current();
                let plugin_name = plugin.clone();
                if let Err(e) = self.create_listeners(vec![(plugin, listener)], &rt) {
                    log::error!("Error while building a metric listener for plugin {plugin_name}: {e:?}");
                }
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
#[derive(thiserror::Error)]
pub enum SendError {
    /// The channel is full.
    #[error("the control channel is full")]
    ChannelFull(ControlMessage),
    /// The task that controls the registry has been shut down.
    ///
    /// This happens when the Alumet pipeline is shut down.
    #[error("the pipeline has been shut down")]
    Shutdown,
}

/// The message could not be sent, or its reply could not be obtained.
#[derive(thiserror::Error, Debug)]
pub enum SendWithReplyError {
    /// The message could not be sent.
    #[error("message could not be sent")]
    Send(SendError),
    /// The message was sent but it was impossible to get a response from the
    /// task that controls the registry.
    #[error("could not get a response from the registry")]
    Recv(oneshot::error::RecvError),
}

// TODO for some method calls, don't return a SendError that can contain the message (it can cause issues with conversion to anyhow::Error)

impl Debug for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChannelFull(msg) => {
                let msg_short: &dyn Debug = &match msg {
                    ControlMessage::RegisterMetrics(_, _, _) => "ControlMessage::RegisterMetrics(...)",
                    ControlMessage::Subscribe(_, _) => "ControlMessage::Subscribe(...)",
                };
                f.debug_tuple("ChannelFull").field(msg_short).finish()
            }
            Self::Shutdown => write!(f, "Shutdown"),
        }
    }
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
    /// # Duplicates
    ///
    /// Duplicates are handled according to the provided [`DuplicateStrategy`].
    ///
    /// If the strategy is `Error`, the registration of each metric will fail if another metric
    /// with the same unique name has already been registered.
    /// For each metric, a `Result<RawMetricId, MetricCreationError>` is returned.
    ///
    /// # Other errors
    /// Regardless of the duplicate strategy, `create_metrics` can also fail if the control message cannot be sent, or if the registration reply cannot be received.
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
    pub fn try_subscribe<F: MetricListenerBuilder + Send + 'static>(
        &self,
        plugin: PluginName,
        listener_builder: F,
    ) -> Result<(), SendError> {
        self.try_send(ControlMessage::Subscribe(plugin, Box::new(listener_builder)))
    }

    /// Adds a new metric listener. Waits until there is capacity to send the message.
    pub async fn subscribe<F: MetricListenerBuilder + Send + 'static>(
        &self,
        plugin: PluginName,
        listener_builder: F,
    ) -> Result<(), SendError> {
        self.send(ControlMessage::Subscribe(plugin, Box::new(listener_builder)))
            .await
    }
}
