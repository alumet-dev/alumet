use std::{
    future::Future,
    io,
    time::{Duration, Instant},
};

use alumet::{
    measurement::MeasurementBuffer,
    metrics::{Metric, RawMetricId},
    pipeline::{
        elements::output::{AsyncOutputStream, StreamRecvError},
        registry::MetricReader,
    },
};
use futures::StreamExt;
use tokio::{net::TcpStream, sync::mpsc};

use crate::{client::retry::RetryState, protocol, serde_impl};

use super::retry::ExponentialRetryPolicy;

/// Exports Alumet measurements to a relay server via TCP.
pub struct TcpOutput {
    settings: Settings,
    alumet: AlumetLink,
    out_relay: protocol::MessageStream<TcpStream>,
    buffer: MeasurementBuffer,
    buffer_last_send: Instant,
}

/// Links between the Alumet pipeline and the relay output.
pub struct AlumetLink {
    /// Stream of measurements.
    pub in_measurements: AsyncOutputStream,
    /// Stream of new metrics.
    pub in_metrics: mpsc::UnboundedReceiver<Vec<(RawMetricId, Metric)>>,
    /// Read-only access to the metric registry.
    pub metrics_reader: MetricReader,
}

/// Settings of the relay output.
pub struct Settings {
    pub client_name: String,
    pub server_address: String,
    pub buffer: BufferSettings,
    pub msg_retry: ExponentialRetryPolicy,
    pub init_retry: ExponentialRetryPolicy,
}

pub struct BufferSettings {
    pub initial_capacity: usize,
    pub max_length: usize,
    pub timeout: Duration,
}

pub enum RetryAction {
    /// Fail immediately and propagate the error.
    Fail,
    /// Retry the current operation.
    RetryOp,
    /// Drop the current TCP connection and reconnect (with handshake and all).
    Reconnect,
}

impl TcpOutput {
    /// Opens a connection to a remote relay server.
    pub async fn connect(alumet: AlumetLink, settings: Settings) -> Result<TcpOutput, protocol::Error> {
        log::info!("Connecting to relay server {}...", settings.server_address);

        // --- connecting
        let mut retry_state = RetryState::new(&settings.init_retry);
        let mut res = connect_to_server(&settings.server_address, &settings.client_name, &alumet.metrics_reader).await;
        while let Err(e) = res {
            if !retry_state.can_retry() {
                return Err(e);
            }
            log::error!("Connection failed: {e:?} - retrying...");
            retry_state.after_attempt().await;
            match retry_action(&e) {
                RetryAction::Fail => return Err(e),
                RetryAction::RetryOp | RetryAction::Reconnect => {
                    res = connect_to_server(&settings.server_address, &settings.client_name, &alumet.metrics_reader)
                        .await;
                }
            }
        }
        // ---

        let out_relay = res.unwrap();
        log::info!("Successfully connected to relay server.");

        // Create a buffer for sending measurements in a more efficient way.
        let buffer = MeasurementBuffer::with_capacity(settings.buffer.initial_capacity);

        Ok(TcpOutput {
            settings,
            alumet,
            out_relay,
            buffer,
            buffer_last_send: Instant::now(),
        })
    }

    /// Serialize the measurements and send the result via TCP.
    async fn send_measurements(&mut self, mut measurements: MeasurementBuffer) -> Result<(), protocol::Error> {
        let now = Instant::now();
        let size_limit_reached = self.buffer.len() + measurements.len() > self.settings.buffer.max_length;
        let timeout_expired = (now - self.buffer_last_send) > self.settings.buffer.timeout;

        log::trace!("size_limit_reached={size_limit_reached}, timeout_expired={timeout_expired}, now={now:?}");

        if !size_limit_reached {
            self.buffer.merge(&mut measurements);
        }
        // TODO it would be better to use a transform step for the buffering, wouldn't it?

        if size_limit_reached || timeout_expired {
            self.buffer_last_send = now;
            let msg = protocol::MessageBody {
                sender: self.settings.client_name.clone(),
                content: protocol::MessageEnum::SendMeasurements(protocol::SendMeasurements {
                    buf: serde_impl::SerdeMeasurementBuffer::Borrowed(&self.buffer),
                }),
            };
            // --- writing
            let mut retry_state = RetryState::new(&self.settings.msg_retry);
            let mut res = self.out_relay.write_message(&msg).await;
            while let Err(e) = res {
                if !retry_state.can_retry() {
                    return Err(e);
                }
                log::error!("Sending measurements failed: {e:?} - retrying...");
                retry_state.after_attempt().await;
                match retry_action(&e) {
                    RetryAction::Fail => return Err(e),
                    RetryAction::RetryOp => res = self.out_relay.write_message(&msg).await,
                    RetryAction::Reconnect => {
                        res = async {
                            self.out_relay = connect_to_server(
                                &self.settings.server_address,
                                &self.settings.client_name,
                                &self.alumet.metrics_reader,
                            )
                            .await?;
                            self.out_relay.write_message(&msg).await
                        }
                        .await;
                    }
                }
            }
            // ---
            self.buffer.clear();
            if size_limit_reached {
                self.buffer.merge(&mut measurements);
            }
        }
        Ok(())
    }

    /// Sends metric definitions via TCP.
    async fn send_metrics(&mut self, metrics_buf: &mut Vec<Vec<(RawMetricId, Metric)>>) -> Result<(), protocol::Error> {
        let iterable = metrics_buf.drain(..).flatten();
        let to_send: Vec<_> = iterable.into_iter().map(protocol::Metric::from).collect();

        let msg = protocol::MessageBody {
            sender: self.settings.client_name.clone(),
            content: protocol::MessageEnum::RegisterMetrics(protocol::RegisterMetrics { metrics: to_send }),
        };

        // NOTE: To make this code generic on the operation, we need either a macro,
        // or the upcoming async closures (https://github.com/rust-lang/rust/pull/132706).
        let mut retry_state = RetryState::new(&self.settings.msg_retry);
        let mut res = self.out_relay.write_message(&msg).await;
        while let Err(e) = res {
            if !retry_state.can_retry() {
                return Err(e);
            }
            log::error!("Sending metrics failed: {e:?} - retrying...");
            retry_state.after_attempt().await;
            match retry_action(&e) {
                RetryAction::Fail => return Err(e),
                RetryAction::RetryOp => res = self.out_relay.write_message(&msg).await,
                RetryAction::Reconnect => {
                    res = async {
                        self.out_relay = connect_to_server(
                            &self.settings.server_address,
                            &self.settings.client_name,
                            &self.alumet.metrics_reader,
                        )
                        .await?;
                        self.out_relay.write_message(&msg).await
                    }
                    .await;
                }
            }
        }
        Ok(())
    }

    /// Continuously polls new measurements and metrics, and sends them via TCP.
    pub fn send_loop(mut self) -> impl Future<Output = anyhow::Result<()>> + Send {
        async move {
            // Handle new measurements and new metrics as they arrive.
            loop {
                // TODO Note: we could avoid the select! here, for example by storing a "metric registry phase id"
                // in the MeasurementBuffer and making sure that the current known phase matches the phase
                // used to produce the measurements (= the registry known by the server is up to date for
                // this measurement buffer).
                let mut metrics_buf = Vec::with_capacity(8);
                tokio::select! {
                    biased;
                    n_metrics = self.alumet.in_metrics.recv_many(&mut metrics_buf, 8) => {
                        if n_metrics == 0 {
                            log::trace!("in_metrics closed => stopping the TcpOutput");
                            break; // the metrics channel has been closed, which means that Alumet is shutting down
                        }
                        self.send_metrics(&mut metrics_buf).await?;
                    }
                    measurements = self.alumet.in_measurements.0.next() => {
                        match measurements {
                            Some(Ok(buf)) => self.send_measurements(buf).await?,
                            Some(Err(StreamRecvError::Lagged(n))) => {
                                log::warn!("{n} measurement buffers were lost because this output was too slow!");
                            }
                            Some(Err(e)) => {
                                log::error!("unexpected error in async TCP-based relay output: {e:?}");
                            }
                            None => {
                                // When the measurement channel closes, it's time to stop.
                                log::trace!("in_measurements closed => stopping the TcpOutput");
                                break
                            }
                        };
                    },
                };
            }
            Ok(())
        }
    }
}

fn retry_action(err: &protocol::Error) -> RetryAction {
    match err {
        protocol::Error::Io(error) => {
            match error.kind() {
                io::ErrorKind::Interrupted => {
                    // no need to reconnect, just retry
                    RetryAction::RetryOp
                }
                io::ErrorKind::InvalidInput
                | io::ErrorKind::PermissionDenied
                | io::ErrorKind::Unsupported
                | io::ErrorKind::OutOfMemory => {
                    // this should not happen unless there's a mistake on our side => don't retry
                    RetryAction::Fail
                }
                _ => {
                    // reconnect and try again
                    RetryAction::Reconnect
                }
            }
        }
        _ => RetryAction::Fail,
    }
}

#[must_use]
async fn connect_to_server(
    server_addr: &str,
    client_name: &str,
    metrics_reader: &MetricReader,
) -> Result<protocol::MessageStream<TcpStream>, protocol::Error> {
    // open the TCP connection
    log::debug!("Opening TCP connection...");
    let stream = TcpStream::connect(server_addr).await?;

    // do the protocol handshake
    log::debug!("Doing protocol handshake...");
    let mut stream = handshake_client2server(client_name.to_owned(), stream).await?;

    // send the metric definitions (for metrics that are known at this point)
    log::debug!("Sending initial metrics...");
    let metrics = metrics_reader.read().await;
    let to_send = metrics
        .iter()
        .map(|(id, def)| protocol::Metric::from((*id, def.to_owned())))
        .collect();
    let msg = protocol::MessageBody {
        sender: client_name.to_owned(),
        content: protocol::MessageEnum::RegisterMetrics(protocol::RegisterMetrics { metrics: to_send }),
    };
    stream.write_message(&msg).await?;

    // done
    Ok(stream)
}

async fn handshake_client2server(
    client_name: String,
    stream: TcpStream,
) -> Result<protocol::MessageStream<TcpStream>, protocol::Error> {
    let mut out_relay = protocol::MessageStream::new(stream);

    // send greeting
    out_relay
        .write_message(&protocol::MessageBody {
            sender: client_name,
            content: protocol::MessageEnum::Greet(protocol::Greet {
                alumet_core_version: String::from(alumet::VERSION),
                relay_plugin_version: String::from(crate::PLUGIN_VERSION),
                protocol_version: protocol::PROTOCOL_VERSION,
            }),
        })
        .await?;

    // receive response
    let response = out_relay.read_message().await?;

    // check compatibility
    if let protocol::MessageEnum::GreetResponse(response) = response.content {
        if response.accept {
            log::info!(
                "Connected to Alumet relay server running Alumet v{}, relay plugin v{}, protocol version {}.",
                response.server_alumet_core_version,
                response.server_relay_plugin_version,
                response.protocol_version
            );
            Ok(out_relay)
        } else {
            log::error!(
                "Cannot connect: client and server are incompatible.
                Client: Alumet v{}, \trelay plugin v{}, \tprotocol version {}
                Server: Alumet v{}, \trelay plugin v{}, \tprotocol version {}",
                alumet::VERSION,
                crate::PLUGIN_VERSION,
                protocol::PROTOCOL_VERSION,
                response.server_alumet_core_version,
                response.server_relay_plugin_version,
                response.protocol_version
            );
            Err(protocol::Error::VersionMismatch {
                client_protocol_version: protocol::PROTOCOL_VERSION,
                server_protocol_version: response.protocol_version,
            })
        }
    } else {
        log::error!("Cannot connect: received unexpected response from server: {response:?}");
        Err(protocol::Error::Unexpected)
    }
}
