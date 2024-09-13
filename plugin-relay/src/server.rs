use std::{
    net::SocketAddr,
    time::{Duration, UNIX_EPOCH},
};

use alumet::{
    measurement::{
        AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType, WrappedMeasurementValue,
    },
    metrics::{Metric, RawMetricId},
    pipeline::{builder::elements::AutonomousSourceRegistration, registry},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable,
    },
    resources::{InvalidConsumerError, InvalidResourceError, Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tonic::{transport::Server, Response, Status};

use crate::{
    protocol::{
        self,
        metric_collector_server::{MetricCollector, MetricCollectorServer},
        register_reply::IdMapping,
        Empty, RegisterReply,
    },
    resolve_socket_address,
};

pub struct RelayServerPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
struct Config {
    /// Address to listen on.
    /// The default value is ip6-localhost = `::1`.
    ///
    /// To listen all your network interfaces please use `0.0.0.0` or `::`.
    address: String,

    /// Port on which to serve.
    port: u16,

    /// IPv6 scope id, for link-local addressing.
    ipv6_scope_id: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: String::from("::1"), // "any" on ipv6
            port: 50051,
            ipv6_scope_id: None,
        }
    }
}

impl AlumetPlugin for RelayServerPlugin {
    fn name() -> &'static str {
        "relay-server"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;

        Ok(Box::new(RelayServerPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // Resolve the address from the config.
        let config_address = std::mem::take(&mut self.config.address);
        let addr: SocketAddr = resolve_socket_address(config_address, self.config.port, self.config.ipv6_scope_id)?[0];

        log::info!("Starting gRPC server with on socket {addr}");
        alumet.add_autonomous_source_builder(move |ctx, cancel_token, out_tx| {
            let metrics_tx = ctx.metrics_sender();
            let collector = GrpcMetricCollector { out_tx, metrics_tx };
            let source = Box::pin(async move {
                Server::builder()
                    .add_service(MetricCollectorServer::new(collector))
                    .serve_with_shutdown(addr, cancel_token.cancelled_owned())
                    .await
                    .context("gRPC server error")?;
                Ok(())
            });
            Ok(AutonomousSourceRegistration {
                name: ctx.source_name("grpc-server"),
                source,
            })
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // The autonomous source has already been stopped at this point.
        Ok(())
    }
}

pub struct GrpcMetricCollector {
    out_tx: tokio::sync::mpsc::Sender<MeasurementBuffer>,
    metrics_tx: registry::MetricSender,
}

#[tonic::async_trait]
impl MetricCollector for GrpcMetricCollector {
    /// Handles a gRPC request to ingest new measurement points.
    async fn ingest_measurements(
        &self,
        request: tonic::Request<crate::protocol::MeasurementBuffer>,
    ) -> Result<Response<Empty>, Status> {
        fn read_measurement(m: protocol::MeasurementPoint) -> anyhow::Result<MeasurementPoint> {
            fn read_attribute(attr: protocol::MeasurementAttribute) -> anyhow::Result<(String, AttributeValue)> {
                let value = attr
                    .value
                    .with_context(|| format!("missing attribute value for key {}", attr.key))?;
                Ok((attr.key, value.into()))
            }

            let timestamp = Timestamp::from(UNIX_EPOCH + Duration::new(m.timestamp_secs, m.timestamp_nanos));
            let value = m.value.context("missing value")?.into();
            let resource = Resource::try_from(m.resource.context("missing resource")?).unwrap();
            let consumer = ResourceConsumer::try_from(m.consumer.context("missing resource consumer")?).unwrap();
            let attributes: anyhow::Result<Vec<_>> = m.attributes.into_iter().map(read_attribute).collect();
            let metric = RawMetricId::from_u64(m.metric);
            Ok(MeasurementPoint::new_untyped(timestamp, metric, resource, consumer, value).with_attr_vec(attributes?))
        }

        // Transform gRPC structures into ALUMET data points.
        let measurements: anyhow::Result<Vec<MeasurementPoint>> =
            request.into_inner().points.into_iter().map(read_measurement).collect();
        let measurements = measurements.map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Send the measurements to the rest of the pipeline.
        self.out_tx.send(MeasurementBuffer::from(measurements)).await.unwrap();

        // Done.
        Ok(Response::new(Empty {}))
    }

    /// Handles a gRPC request to register new metrics.
    async fn register_metrics(
        &self,
        request: tonic::Request<crate::protocol::MetricDefinitions>,
    ) -> Result<Response<RegisterReply>, Status> {
        fn read_metric(m: crate::protocol::metric_definitions::MetricDef) -> anyhow::Result<(u64, Metric)> {
            let value_type = protocol::MeasurementValueType::try_from(m.r#type)?.into();
            let metric = Metric {
                name: m.name,
                description: m.description,
                value_type,
                unit: m.unit.context("missing unit")?.try_into()?,
            };
            Ok((m.id_for_agent, metric))
        }

        // Extract the client name from metadata header, or address.
        let client_name = request
            .metadata()
            .get("x-alumet-client")
            .and_then(|v| v.to_str().ok().map(|s| s.to_owned()))
            .or_else(|| request.remote_addr().map(|addr| addr.to_string()))
            .unwrap_or_else(|| String::from("?"));

        // Read the incoming metric definitions.
        let defs = request.into_inner().definitions;
        let mut client_metrics_ids = Vec::with_capacity(defs.len());
        let mut metrics = Vec::with_capacity(defs.len());
        for incoming_metric in defs {
            let (client_id, alumet_metric) =
                read_metric(incoming_metric).map_err(|e| Status::invalid_argument(e.to_string()))?;
            client_metrics_ids.push(client_id);
            metrics.push(alumet_metric);
        }

        // Attempt to register the metrics.
        let server_metric_ids = self
            .metrics_tx
            .create_metrics(metrics, registry::DuplicateStrategy::Rename { suffix: client_name })
            .await
            .map_err(|e| Status::internal(format!("create_metrics failed: {e}")))?;

        // Maps client ids to server ids.
        let mappings = client_metrics_ids
            .into_iter()
            .zip(server_metric_ids)
            .map(|(client_id, server_id)| match server_id {
                Ok(id) => Ok(IdMapping {
                    id_for_agent: client_id,
                    id_for_collector: id.as_u64(),
                }),
                Err(e) => Err(Status::internal(e.to_string())),
            })
            .collect::<Result<Vec<IdMapping>, Status>>()?;

        // Send the response (happy path).
        Ok(Response::new(RegisterReply { mappings }))
    }
}

impl From<protocol::MeasurementValueType> for WrappedMeasurementType {
    fn from(value: protocol::MeasurementValueType) -> Self {
        match value {
            protocol::MeasurementValueType::U64 => WrappedMeasurementType::U64,
            protocol::MeasurementValueType::F64 => WrappedMeasurementType::F64,
        }
    }
}

impl TryFrom<protocol::PrefixedUnit> for PrefixedUnit {
    type Error = anyhow::Error;

    fn try_from(value: protocol::PrefixedUnit) -> Result<Self, Self::Error> {
        Ok(PrefixedUnit {
            base_unit: value.base_unit.parse().unwrap_or_else(|_| Unit::Custom {
                unique_name: value.base_unit.clone(),
                display_name: format!("/{}/", value.base_unit),
            }),
            prefix: value.prefix.parse()?,
        })
    }
}

impl From<protocol::measurement_point::Value> for WrappedMeasurementValue {
    fn from(value: protocol::measurement_point::Value) -> Self {
        match value {
            protocol::measurement_point::Value::U64(v) => WrappedMeasurementValue::U64(v),
            protocol::measurement_point::Value::F64(v) => WrappedMeasurementValue::F64(v),
        }
    }
}

impl From<protocol::measurement_attribute::Value> for AttributeValue {
    fn from(value: protocol::measurement_attribute::Value) -> Self {
        match value {
            protocol::measurement_attribute::Value::Str(v) => AttributeValue::String(v),
            protocol::measurement_attribute::Value::U64(v) => AttributeValue::U64(v),
            protocol::measurement_attribute::Value::F64(v) => AttributeValue::F64(v),
            protocol::measurement_attribute::Value::Bool(v) => AttributeValue::Bool(v),
        }
    }
}

impl TryFrom<protocol::Resource> for Resource {
    type Error = InvalidResourceError;

    fn try_from(value: protocol::Resource) -> Result<Self, Self::Error> {
        match value.id {
            Some(id) => Resource::parse(value.kind, id),
            None => match value.kind.as_str() {
                "local_machine" => Ok(Resource::LocalMachine),
                wrong => Err(InvalidResourceError::InvalidId(wrong.to_owned().into())),
            },
        }
    }
}

impl TryFrom<protocol::ResourceConsumer> for ResourceConsumer {
    type Error = InvalidConsumerError;

    fn try_from(value: protocol::ResourceConsumer) -> Result<Self, Self::Error> {
        match value.id {
            Some(id) => ResourceConsumer::parse(value.kind, id),
            None => match value.kind.as_str() {
                "local_machine" => Ok(ResourceConsumer::LocalMachine),
                wrong => Err(InvalidConsumerError::InvalidId(wrong.to_owned().into())),
            },
        }
    }
}
