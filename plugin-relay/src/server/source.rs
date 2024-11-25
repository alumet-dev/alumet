use std::{future::Future, net::SocketAddr};

use alumet::{measurement::MeasurementBuffer, metrics::Metric, pipeline::registry::MetricSender};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

use crate::protocol::{self, GreetResponse, MessageBody, MessageEnum, MessageStream, PROTOCOL_VERSION};

use super::metrics::MetricConverter;

pub struct TcpSource {
    cancel_token: CancellationToken,
    tcp: MessageStream<TcpStream>,
    out_tx: mpsc::Sender<MeasurementBuffer>,
    metrics: MetricConverter,
}

pub struct TcpServer {
    cancel_token: CancellationToken,
    listener: TcpListener,
    measurement_tx: mpsc::Sender<MeasurementBuffer>,
    metrics_tx: MetricSender,
}

impl TcpSource {
    async fn process_message(&mut self, msg: MessageBody) -> anyhow::Result<()> {
        match msg.content {
            MessageEnum::Greet(greet) => {
                // Ensure that the server is compatible and respond.
                log::debug!("Received {greet:?}");
                let accept = greet.protocol_version == PROTOCOL_VERSION; // TODO check alumet and plugin are compatible?
                if !accept {
                    let remote_addr = self
                        .tcp
                        .peer_addr()
                        .map_or_else(|err| format!("? ({err})"), |s| s.to_string());
                    log::warn!("Client {remote_addr} uses protocol version {}, which is not compatible with our protocol version {}. Rejecting.", greet.protocol_version, PROTOCOL_VERSION);
                    return Ok(());
                }
                self.tcp
                    .write_message(MessageBody {
                        sender: String::from(""),
                        content: MessageEnum::GreetResponse(GreetResponse {
                            accept,
                            server_alumet_core_version: alumet::VERSION.to_string(),
                            server_relay_plugin_version: crate::PLUGIN_VERSION.to_string(),
                            protocol_version: PROTOCOL_VERSION,
                        }),
                    })
                    .await?;
            }
            MessageEnum::RegisterMetrics(register_metrics) => {
                let mut metric_ids = Vec::with_capacity(register_metrics.metrics.len());
                let mut metric_defs = Vec::with_capacity(register_metrics.metrics.len());
                for protocol_metric in register_metrics.metrics {
                    let alumet_metric = Metric {
                        name: protocol_metric.name,
                        description: String::from("remote metric via plugin_relay"),
                        value_type: protocol_metric.value_type.try_into()?,
                        unit: protocol_metric.unit.try_into()?,
                    };
                    metric_defs.push(alumet_metric);
                    metric_ids.push(protocol_metric.id);
                }
                self.metrics.register_from_client(metric_ids, metric_defs).await?;
            }
            MessageEnum::SendMeasurements(send_measurements) => {
                let mut alumet_measurements = send_measurements.buf.0;
                // convert the metrics
                self.metrics.convert_all(&mut alumet_measurements)?;
                // send them
                self.out_tx.send(alumet_measurements).await?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    pub fn receive_loop(mut self) -> impl Future<Output = anyhow::Result<()>> + Send {
        fn is_fatal_error(err: &protocol::Error) -> bool {
            match err {
                protocol::Error::Io(_) => true,
                protocol::Error::Serde(error) => match error {
                    postcard::Error::WontImplement
                    | postcard::Error::NotYetImplemented
                    | postcard::Error::SerializeBufferFull
                    | postcard::Error::SerializeSeqLengthUnknown
                    | postcard::Error::SerdeSerCustom => true,
                    _ => false,
                },
                protocol::Error::Disconnected => false,
            }
        }

        async move {
            loop {
                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        break;
                    },
                    message = self.tcp.read_message() => {
                        match message {
                            Ok(msg) => {
                                self.process_message(msg).await?;
                            },
                            Err(protocol::Error::Disconnected) => {
                                // stop the loop normally
                                break;
                            },
                            Err(err) => {
                                if is_fatal_error(&err) {
                                    // stop the loop with an error
                                    return Err(err.into());
                                } else {
                                    // try to continue (TODO maybe we should not do this?)
                                    log::error!("error while processing message from client: {err:?}");
                                }
                            },
                        };
                    }
                }
            }
            Ok(())
        }
    }
}

impl TcpServer {
    pub fn new(
        cancel_token: CancellationToken,
        listener: TcpListener,
        measurement_tx: mpsc::Sender<MeasurementBuffer>,
        metrics_tx: MetricSender,
    ) -> Self {
        Self {
            cancel_token,
            listener,
            measurement_tx,
            metrics_tx,
        }
    }

    fn start_receiving(&mut self, tcp_stream: TcpStream, remote_addr: SocketAddr) {
        log::info!("Accepting new client {remote_addr}");
        let source = TcpSource {
            cancel_token: self.cancel_token.child_token(),
            tcp: MessageStream::new(tcp_stream),
            out_tx: self.measurement_tx.clone(),
            metrics: MetricConverter::new(self.metrics_tx.clone(), format!("relay-client-{remote_addr}")),
        };
        tokio::spawn(async move {
            if let Err(e) = source.receive_loop().await {
                log::error!("Error in relay source connected to client {remote_addr}: {e:?}");
            }
            log::info!("Client disconnected: {remote_addr}");
        });
    }

    pub fn accept_loop(mut self) -> impl Future<Output = anyhow::Result<()>> + Send {
        async move {
            loop {
                tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        break;
                    }
                    incoming = self.listener.accept() => {
                        match incoming {
                            Ok((tcp_stream, remote_addr)) => {
                                self.start_receiving(tcp_stream, remote_addr);
                            },
                            Err(e) => {
                                log::error!("unexpected error in async TCP listener: {e:?}");
                            }
                        }
                    }

                }
            }
            Ok(())
        }
    }
}