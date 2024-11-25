use std::{
    future::Future,
    time::{Duration, Instant},
};

use alumet::{
    measurement::MeasurementBuffer,
    metrics::{Metric, RawMetricId},
    pipeline::elements::output::{AsyncOutputStream, StreamRecvError},
};
use futures::StreamExt;
use tokio::{net::TcpStream, sync::mpsc};

use crate::{protocol, serde_impl};

/// Exports Alumet measurements to a relay server via TCP.
pub struct TcpOutput {
    settings: TcpOutputSettings,
    in_measurements: AsyncOutputStream,
    in_metrics: mpsc::UnboundedReceiver<Vec<(RawMetricId, Metric)>>,
    out_relay: protocol::MessageStream<TcpStream>,
    buffer: MeasurementBuffer,
    buffer_last_send: Instant,
}

pub struct TcpOutputSettings {
    pub client_name: String,
    pub buffer_initial_capacity: usize,
    pub buffer_max_length: usize,
    pub buffer_timeout: Duration,
}

impl TcpOutput {
    /// Opens a connection to a remote relay server.
    pub async fn connect(
        in_measurements: AsyncOutputStream,
        in_metrics: mpsc::UnboundedReceiver<Vec<(RawMetricId, Metric)>>,
        remote_addr: String,
        settings: TcpOutputSettings,
    ) -> anyhow::Result<TcpOutput> {
        // establish TCP connection
        let stream = TcpStream::connect(remote_addr).await?;
        let mut out_relay = protocol::MessageStream::new(stream);

        // send greeting
        out_relay
            .write_message(protocol::MessageBody {
                sender: settings.client_name.clone(),
                content: protocol::MessageEnum::Greet(protocol::Greet {
                    alumet_core_version: String::from(alumet::VERSION),
                    relay_plugin_version: String::from(crate::PLUGIN_VERSION),
                    protocol_version: protocol::PROTOCOL_VERSION,
                }),
            })
            .await?;
        // receive response
        let response = out_relay.read_message().await?;
        if let protocol::MessageEnum::GreetResponse(response) = response.content {
            if response.accept {
                log::info!(
                    "Connected to Alumet relay server running Alumet v{}, relay plugin v{}, protocol version {}.",
                    response.server_alumet_core_version,
                    response.server_relay_plugin_version,
                    response.protocol_version
                );
            } else {
                return Err(anyhow::anyhow!(
                    "Cannot connect: client and server are incompatible.
                    Client: Alumet v{}, \trelay plugin v{}, \tprotocol version {}
                    Server: Alumet v{}, \trelay plugin v{}, \tprotocol version {}",
                    alumet::VERSION,
                    crate::PLUGIN_VERSION,
                    protocol::PROTOCOL_VERSION,
                    response.server_alumet_core_version,
                    response.server_relay_plugin_version,
                    response.protocol_version,
                ));
            }
        }

        // Create a buffer for sending measurements in a more efficient way.
        let buffer = MeasurementBuffer::with_capacity(settings.buffer_initial_capacity);

        Ok(TcpOutput {
            settings,
            in_measurements,
            in_metrics,
            out_relay,
            buffer,
            buffer_last_send: Instant::now(),
        })
    }

    /// Serialize the measurements and send the result via TCP.
    async fn send_measurements(&mut self, mut measurements: MeasurementBuffer) -> Result<(), protocol::Error> {
        let now = Instant::now();
        let size_limit_reached = self.buffer.len() + measurements.len() > self.settings.buffer_max_length;
        let timeout_expired = (now - self.buffer_last_send) > self.settings.buffer_timeout;

        log::trace!("size_limit_reached={size_limit_reached}, timeout_expired={timeout_expired}, now={now:?}");

        if !size_limit_reached {
            self.buffer.merge(&mut measurements);
        }
        // TODO it would be better to use a transform step for the buffering, wouldn't it?

        if size_limit_reached || timeout_expired {
            self.buffer_last_send = now;
            self.out_relay
                .write_message(protocol::MessageBody {
                    sender: self.settings.client_name.clone(),
                    content: protocol::MessageEnum::SendMeasurements(protocol::SendMeasurements {
                        buf: serde_impl::SerdeMeasurementBuffer::Borrowed(&self.buffer),
                    }),
                })
                .await?;
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
        let to_send = iterable.into_iter().map(|(id, def)| protocol::Metric {
            id: id.as_u64(),
            name: def.name,
            value_type: def.value_type.into(),
            unit: def.unit.into(),
        });
        self.out_relay
            .write_message(protocol::MessageBody {
                sender: self.settings.client_name.clone(),
                content: protocol::MessageEnum::RegisterMetrics(protocol::RegisterMetrics {
                    metrics: to_send.collect(),
                }),
            })
            .await
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
                    n_metrics = self.in_metrics.recv_many(&mut metrics_buf, 8) => {
                        if n_metrics == 0 {
                            break; // the metrics channel has been closed, which means that Alumet is shutting down
                        }
                        self.send_metrics(&mut metrics_buf).await?;
                    }
                    measurements = self.in_measurements.0.next() => {
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
