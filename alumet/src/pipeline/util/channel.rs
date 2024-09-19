//! Abstractions over different kinds of channel.

use futures::Stream;
use tokio::sync::{broadcast, mpsc};

use crate::measurement::MeasurementBuffer;

/// Trait that allows to receive measurements from different kinds of channel.
pub trait MeasurementReceiver {
    async fn recv(&mut self) -> Result<MeasurementBuffer, RecvError>;
    fn into_stream(self) -> impl Stream<Item = Result<MeasurementBuffer, StreamRecvError>>;
}

pub enum ReceiverEnum {
    Broadcast(broadcast::Receiver<MeasurementBuffer>),
    Single(mpsc::Receiver<MeasurementBuffer>),
}

pub struct ReceiverProvider(ProviderEnum);

enum ProviderEnum {
    Broadcast(broadcast::Sender<MeasurementBuffer>),
    Single(Option<mpsc::Receiver<MeasurementBuffer>>),
}

// common error enum

pub enum RecvError {
    Lagged(u64),
    Closed,
}

#[non_exhaustive]
pub enum StreamRecvError {
    Lagged(u64),
}

// receiver implementations

impl MeasurementReceiver for broadcast::Receiver<MeasurementBuffer> {
    async fn recv(&mut self) -> Result<MeasurementBuffer, RecvError> {
        broadcast::Receiver::recv(self).await.map_err(|e| match e {
            broadcast::error::RecvError::Closed => RecvError::Closed,
            broadcast::error::RecvError::Lagged(n) => RecvError::Lagged(n),
        })
    }

    fn into_stream(self) -> impl Stream<Item = Result<MeasurementBuffer, StreamRecvError>> {
        use tokio_stream::wrappers::{errors::BroadcastStreamRecvError, BroadcastStream};
        use tokio_stream::StreamExt;

        BroadcastStream::new(self).map(|item| {
            item.map_err(|e| match e {
                BroadcastStreamRecvError::Lagged(n) => StreamRecvError::Lagged(n),
            })
        })
    }
}

impl MeasurementReceiver for mpsc::Receiver<MeasurementBuffer> {
    async fn recv(&mut self) -> Result<MeasurementBuffer, RecvError> {
        match mpsc::Receiver::recv(self).await {
            Some(buf) => Ok(buf),
            None => Err(RecvError::Closed),
        }
    }

    fn into_stream(self) -> impl Stream<Item = Result<MeasurementBuffer, StreamRecvError>> {
        use tokio_stream::{wrappers::ReceiverStream, StreamExt};
        ReceiverStream::new(self).map(Ok)
    }
}

// providers

impl ReceiverProvider {
    pub fn get(&mut self) -> ReceiverEnum {
        match &mut self.0 {
            ProviderEnum::Broadcast(tx) => ReceiverEnum::Broadcast(tx.subscribe()),
            ProviderEnum::Single(rx) => ReceiverEnum::Single(
                rx.take()
                    .expect("ProviderEnum::get called but the single MeasurementReceiver has already been taken"),
            ),
        }
    }
}

impl From<broadcast::Sender<MeasurementBuffer>> for ReceiverProvider {
    fn from(value: broadcast::Sender<MeasurementBuffer>) -> Self {
        Self(ProviderEnum::Broadcast(value))
    }
}

impl From<mpsc::Receiver<MeasurementBuffer>> for ReceiverProvider {
    fn from(value: mpsc::Receiver<MeasurementBuffer>) -> Self {
        Self(ProviderEnum::Single(Some(value)))
    }
}
