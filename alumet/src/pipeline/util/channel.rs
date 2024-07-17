//! Abstractions over different kinds of channel.

use tokio::sync::{broadcast, mpsc};

use crate::measurement::MeasurementBuffer;

/// Trait that allows to receive measurements from different kinds of channel.
pub trait MeasurementReceiver {
    async fn recv(&mut self) -> Result<MeasurementBuffer, RecvError>;
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

pub enum RecvError {
    Lagged(u64),
    Closed,
}

// receiver implementations

impl MeasurementReceiver for broadcast::Receiver<MeasurementBuffer> {
    async fn recv(&mut self) -> Result<MeasurementBuffer, RecvError> {
        broadcast::Receiver::recv(self).await.map_err(|e| match e {
            broadcast::error::RecvError::Closed => RecvError::Closed,
            broadcast::error::RecvError::Lagged(n) => RecvError::Lagged(n),
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
