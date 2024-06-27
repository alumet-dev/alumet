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

pub enum ControlMessage {
    Register(
        Vec<Metric>,
        DuplicateStrategy,
        Option<oneshot::Sender<Result<Vec<RawMetricId>, MetricCreationError>>>,
    ),
}

pub enum DuplicateStrategy {
    Error,
    Rename { suffix: String },
}

pub(crate) struct MetricRegistryControl {
    registry: Arc<RwLock<MetricRegistry>>,
}

impl MetricRegistryControl {
    pub fn new(registry: MetricRegistry) -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry)),
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
            ControlMessage::Register(metrics, dup, reply_to) => {
                // Use an RCU (Read, Copy, Update) scheme to modify the registry with the minimal
                // amount of blocking for readers and writers.
                //
                // NOTE: Since we don't take the write lock, we must ensure that only one thread
                // is performing the copy and the update (otherwise we would end up with multiple
                // desynchronized copies). This is achieved by handling all the messages in one
                // task, thus making their processing sequential.

                // read and copy
                let mut copy = (*self.registry.read().await).clone();
                // modify the copy
                let res = match dup {
                    DuplicateStrategy::Error => copy.extend(metrics),
                    DuplicateStrategy::Rename { suffix } => Ok(copy.extend_infallible(metrics, &suffix)),
                    // TODO return Vec<Result<Id, Error>> instead of Result<Vec<Id>, Error>
                };
                // update
                *self.registry.write().await = copy;

                // send reply
                if let Some(tx) = reply_to {
                    tx.send(res).expect("failed to send reply");
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
                        None => todo!("registry_control_loop#rx chnanel closed")
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct MetricAccess {
    pub(crate) inner: Arc<RwLock<MetricRegistry>>,
}

#[derive(Clone)]
pub struct MetricReader(MetricAccess);

#[derive(Clone)]
pub struct MetricSender(mpsc::Sender<ControlMessage>);

pub enum ControlError {
    ChannelFull(ControlMessage),
    Shutdown,
}

impl MetricAccess {
    pub async fn read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.inner.read().await
    }
    
    pub fn blocking_read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.inner.blocking_read()
    }

    pub async fn write(&self) -> tokio::sync::RwLockWriteGuard<MetricRegistry> {
        self.inner.write().await
    }

    pub fn into_read_only(self) -> MetricReader {
        MetricReader(self)
    }
}

impl MetricReader {
    pub async fn read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.0.read().await
    }
    
    pub fn blocking_read(&self) -> RwLockReadGuard<MetricRegistry> {
        self.0.blocking_read()
    }
}

impl MetricSender {
    pub async fn send(&mut self, message: ControlMessage) -> Result<(), ControlError> {
        self.0.send(message).await.map_err(|_| ControlError::Shutdown)
    }

    pub fn try_send(&mut self, message: ControlMessage) -> Result<(), ControlError> {
        match self.0.try_send(message) {
            Ok(_) => Ok(()),
            Err(mpsc::error::TrySendError::Full(m)) => Err(ControlError::ChannelFull(m)),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ControlError::Shutdown),
        }
    }
}
