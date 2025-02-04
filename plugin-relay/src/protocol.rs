//! Relay protocol: defines the messages exchanged by the relay client and relay server.

use std::{io, time::Duration};

use alumet::{measurement::WrappedMeasurementType, metrics::RawMetricId, units::PrefixedUnit};
use anyhow::Context;
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::error::Elapsed,
};

use crate::serde_impl;

/// Version number of the current protocol.
///
/// IMPORTANT: you must increase this number when the protocol changes.
pub const PROTOCOL_VERSION: u32 = 2;

/// Maximum size (in bytes) of a message body.
///
/// Messages that are larger are rejected by the server.
pub const MAX_MESSAGE_BODY_SIZE: u32 = 32_000_000; // 32 MB

/// Capacity (in bytes) of the serialization/deserialization buffer.
const BUFFER_CAPACITY: usize = 8192;

// TODO make the header 3 bytes instead of 4

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O error.
    #[error("tcp i/o error")]
    Io(#[from] std::io::Error),

    /// A serde error.
    #[error("(de)serialization error")]
    Serde(#[from] postcard::Error),

    /// EOF at message boundary.
    #[error("peer disconnected")]
    Disconnected,

    /// Incompatible peer.
    #[error("client and server are incompatible: the client uses protocol version {client_protocol_version}, but the server uses protocol version {server_protocol_version}")]
    VersionMismatch {
        client_protocol_version: u32,
        server_protocol_version: u32,
    },

    #[error("received an unexpected response")]
    Unexpected,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageBody<'s> {
    /// The client id or server id.
    pub sender: String,

    /// The content of the message.
    pub content: MessageEnum<'s>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MessageEnum<'s> {
    Greet(Greet),
    GreetResponse(GreetResponse),
    RegisterMetrics(RegisterMetrics),
    SendMeasurements(SendMeasurements<'s>),
}

/// Sent by the client at the beginning of the connection.
#[derive(Debug, Serialize, Deserialize)]
pub struct Greet {
    pub alumet_core_version: String,
    pub relay_plugin_version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GreetResponse {
    pub accept: bool,
    pub server_alumet_core_version: String,
    pub server_relay_plugin_version: String,
    pub protocol_version: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterMetrics {
    pub metrics: Vec<Metric>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metric {
    pub id: u64,
    pub name: String,
    pub value_type: MetricType,
    pub unit: MetricUnit,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricUnit {
    pub base: String,
    pub prefix: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum MetricType {
    F64,
    U64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SendMeasurements<'s> {
    pub buf: serde_impl::SerdeMeasurementBuffer<'s>,
}

/// Allows to read/write protocol messages from/to an asynchronous IO stream.
///
/// # Coherency
/// This stream only handles individual messages, not the whole communication protocol.
/// In particular, it does not implement the client-server handshake.
pub struct MessageStream<S: AsyncRead + AsyncWrite + Unpin> {
    stream: S,
    serializer: postcard::Serializer<OpenVecFlavor>,
    deserialization_buffer: BytesMut,
}

impl<S: AsyncRead + AsyncWrite + Unpin> MessageStream<S> {
    pub fn new(underlying: S) -> Self {
        Self {
            stream: underlying,
            serializer: postcard::Serializer {
                output: OpenVecFlavor::new(Vec::with_capacity(BUFFER_CAPACITY)),
            },
            deserialization_buffer: BytesMut::with_capacity(BUFFER_CAPACITY),
        }
    }

    pub(crate) fn serialize_full_message(&mut self, msg: &MessageBody<'_>) -> Result<(), Error> {
        // reserve 4 bytes for the msg length
        self.serializer.output.bytes.resize(4, 0);

        // serialize the message
        msg.serialize(&mut self.serializer)?;

        // prepend the actual length
        let len = self.serializer.output.bytes.len() - 4;
        let len_bytes = (len as u32).to_be_bytes();
        debug_assert_eq!(len_bytes.len(), 4); // ensure that we obtain 4 bytes
        log::trace!("body length: {len}");
        log::trace!("body to serialize: {msg:?}");

        let header = &mut self.serializer.output.bytes[0..4];
        header.copy_from_slice(&len_bytes);
        Ok(())
    }

    pub async fn write_message<'a>(&'a mut self, msg: &'a MessageBody<'a>) -> Result<(), Error> {
        // serialize to the serializer buffer
        self.serialize_full_message(msg)?;

        // write to the underlying data stream (tcp socket)
        self.stream.write_all(&self.serializer.output.bytes).await?;
        self.serializer.output.bytes.clear();
        Ok(())
    }

    pub async fn read_timeout(&mut self, timeout: Duration) -> Result<Result<MessageBody<'static>, Error>, Elapsed> {
        tokio::time::timeout(timeout, self.read_message()).await
    }

    pub async fn read_message(&mut self) -> Result<MessageBody<'static>, Error> {
        // First, deserialize the next message header. We need 4 bytes.
        // Then, deserialize the message body.

        // Read from the tcp socket until we get 4 bytes
        let mut header_read = 0;
        while header_read < 4 {
            let n = self.stream.read_buf(&mut self.deserialization_buffer).await?;
            header_read += n;
            if n == 0 {
                if header_read == 0 {
                    return Err(Error::Disconnected);
                } else {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
                }
            }
        }
        // Parse the header: it's just the length of the message body
        let body_len_bytes: [u8; 4] = self.deserialization_buffer[0..4].try_into().unwrap();
        let body_len = u32::from_be_bytes(body_len_bytes);
        log::trace!(
            "body length: {body_len}; already in the buffer: {}",
            self.deserialization_buffer.len()
        );

        // Prevent DOS attack or invalid length.
        if body_len > MAX_MESSAGE_BODY_SIZE {
            let msg = format!("message too big: body length is {body_len} but it should be less than the maximum allowed {MAX_MESSAGE_BODY_SIZE}");
            return Err(io::Error::new(io::ErrorKind::InvalidData, msg).into());
        }

        // Ensure that we have enough capacity for the entire message.
        let message_len = (body_len as usize) + 4;
        if let Some(additional) = message_len.checked_sub(self.deserialization_buffer.capacity()) {
            self.deserialization_buffer.reserve(additional);
        }

        // Read more data if required.
        while self.deserialization_buffer.len() < message_len {
            self.stream.read_buf(&mut self.deserialization_buffer).await?;
        }

        // Take the data
        //
        // Before:
        // [hhhh BBBBBBBBBB xxxxxxxxxxxxxxxx| ________]
        //    ^      ^           ^          |    ^    |
        // header   body   more bytes read  |  empty  |
        // (4 bytes)                        |         |
        //                               buffer     buffer
        //                               length    capacity
        //
        // After:
        // - message_bytes = [hhhh BBBBBBBBBB] (header and body)
        //                                   |
        //                          buffer length
        //
        //                      ^^^^^^^^^^
        //                   &message_bytes[4..]
        //
        // - deserialization_buffer = [xxxxxxxxxxxxxxxx _____] (remaining data and empty space)
        //                                             |
        //                                       buffer length
        //
        let message_bytes = self.deserialization_buffer.split_to(message_len);
        let body_bytes = &message_bytes[4..]; // body = message without the header
        debug_assert_eq!(body_bytes.len(), body_len as usize);
        log::trace!("body bytes: {body_bytes:?}");

        // Deserialize the message body (skipping the header). Note: this could be done on another thread/task.
        let (body_msg, unused_bytes): (MessageBody, &[u8]) = postcard::take_from_bytes(body_bytes)?;
        if !unused_bytes.is_empty() {
            log::warn!(
                "{} unused bytes after decoded message body {:?}. This is probably a bug.",
                unused_bytes.len(),
                body_msg
            );
        }
        log::trace!("deserialized body: {body_msg:?}");

        // At the end of this function, `body_bytes` is dropped and the corresponding space in the buffer
        // can be reused.
        Ok(body_msg)
    }
}

impl MessageStream<TcpStream> {
    pub fn peer_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.peer_addr()
    }

    pub fn local_addr(&self) -> Result<std::net::SocketAddr, std::io::Error> {
        self.stream.local_addr()
    }

    pub async fn shutdown(&mut self) -> Result<(), std::io::Error> {
        self.stream.shutdown().await
    }
}

struct OpenVecFlavor {
    bytes: Vec<u8>,
}

impl OpenVecFlavor {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl postcard::ser_flavors::Flavor for OpenVecFlavor {
    type Output = Vec<u8>;

    #[inline(always)]
    fn try_push(&mut self, data: u8) -> postcard::Result<()> {
        self.bytes.push(data);
        Ok(())
    }

    #[inline(always)]
    fn try_extend(&mut self, data: &[u8]) -> postcard::Result<()> {
        self.bytes.extend_from_slice(data);
        Ok(())
    }

    fn finalize(self) -> postcard::Result<Self::Output> {
        Ok(self.bytes)
    }
}

impl From<PrefixedUnit> for MetricUnit {
    fn from(value: PrefixedUnit) -> Self {
        Self {
            base: value.base_unit.unique_name().to_owned(),
            prefix: value.prefix.unique_name().to_owned(),
        }
    }
}

impl TryFrom<MetricUnit> for PrefixedUnit {
    type Error = anyhow::Error;

    fn try_from(value: MetricUnit) -> Result<Self, Self::Error> {
        Ok(Self {
            base_unit: value
                .base
                .parse()
                .with_context(|| format!("invalid base unit {}", value.base))?,
            prefix: value
                .prefix
                .parse()
                .with_context(|| format!("invalid unit prefix {}", value.prefix))?,
        })
    }
}

impl From<WrappedMeasurementType> for MetricType {
    fn from(value: WrappedMeasurementType) -> Self {
        match value {
            WrappedMeasurementType::F64 => MetricType::F64,
            WrappedMeasurementType::U64 => MetricType::U64,
        }
    }
}

impl From<MetricType> for WrappedMeasurementType {
    fn from(value: MetricType) -> Self {
        match value {
            MetricType::F64 => WrappedMeasurementType::F64,
            MetricType::U64 => WrappedMeasurementType::U64,
        }
    }
}

impl From<(RawMetricId, alumet::metrics::Metric)> for Metric {
    fn from(value: (RawMetricId, alumet::metrics::Metric)) -> Self {
        let (id, def) = value;
        Self {
            id: id.as_u64(),
            name: def.name,
            value_type: def.value_type.into(),
            unit: def.unit.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::BytesMut;

    use super::{MessageBody, MessageStream, PROTOCOL_VERSION};

    #[test]
    fn test_message_rw_simple() -> anyhow::Result<()> {
        // TODO
        Ok(())
    }
}
