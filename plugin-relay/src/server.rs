use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    time::{Duration, UNIX_EPOCH},
};

use alumet::{
    measurement::{
        AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType, WrappedMeasurementValue,
    },
    metrics::{Metric, RawMetricId},
    pipeline::builder::LateRegistrationHandle,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetStart, ConfigTable,
    },
    resources::{InvalidConsumerError, InvalidResourceError, Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tonic::{transport::Server, Response, Status};

use crate::protocol::{
    self,
    metric_collector_server::{MetricCollector, MetricCollectorServer},
    register_reply::IdMapping,
    Empty, RegisterReply,
};

pub struct RelayServerPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
struct Config {
    /// Port on which to serve.
    port: u16,

    /// If true, bind the server to an IPv4 socket instead of IPv6.
    ipv4_only: bool,

    /// IPv6 scope id, for link-local addressing.
    ipv6_scope_id: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 50051,
            ipv4_only: false,
            ipv6_scope_id: None,
        }
    }
}

impl AlumetPlugin for RelayServerPlugin {
    fn name() -> &'static str {
        "plugin-relay:server"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(RelayServerPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
        let addr: SocketAddr = match self.config.ipv4_only {
            true => SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.config.port)),
            false => SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::LOCALHOST,
                self.config.port,
                0,
                self.config.ipv6_scope_id.unwrap_or(0),
            )),
        };
        log::info!("Starting gRPC server with on socket {addr}");
        alumet.add_autonomous_source(move |p, cancel_token, out_tx| {
            let late_reg = tokio::sync::Mutex::new(p.late_registration_handle());
            let collector = GrpcMetricCollector { out_tx, late_reg };
            async move {
                Server::builder()
                    .add_service(MetricCollectorServer::new(collector))
                    .serve_with_shutdown(addr, cancel_token.cancelled_owned())
                    .await
                    .context("server error")?;
                Ok(())
            }
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

pub struct GrpcMetricCollector {
    out_tx: tokio::sync::mpsc::Sender<MeasurementBuffer>,
    late_reg: tokio::sync::Mutex<LateRegistrationHandle>,
}

#[tonic::async_trait]
impl MetricCollector for GrpcMetricCollector {
    async fn ingest_measurements(
        &self,
        request: tonic::Request<crate::protocol::MeasurementBuffer>,
    ) -> Result<Response<Empty>, Status> {
        // TODO proper error handling

        // Transform gRPC structures into ALUMET data points.
        let measurements: Vec<MeasurementPoint> = request
            .into_inner()
            .points
            .into_iter()
            .map(|m| {
                let timestamp = Timestamp::from(UNIX_EPOCH + Duration::new(m.timestamp_secs, m.timestamp_nanos));
                let value = m.value.unwrap().into();
                let resource = Resource::try_from(m.resource.unwrap()).unwrap();
                let consumer = ResourceConsumer::try_from(m.consumer.unwrap()).unwrap();
                let attributes: Vec<_> = m
                    .attributes
                    .into_iter()
                    .map(|attr| (attr.key, attr.value.unwrap().into()))
                    .collect();
                MeasurementPoint::new_untyped(timestamp, RawMetricId::from_u64(m.metric), resource, consumer, value)
                    .with_attr_vec(attributes)
            })
            .collect();

        // Send the measurements to the rest of the pipeline.
        self.out_tx.send(MeasurementBuffer::from(measurements)).await.unwrap();

        // Done.
        Ok(Response::new(Empty {}))
    }

    async fn register_metrics(
        &self,
        request: tonic::Request<crate::protocol::MetricDefinitions>,
    ) -> Result<Response<RegisterReply>, Status> {
        // TODO convert errors to a proper Status
        let client_name = request
            .metadata()
            .get("x-alumet-client")
            .and_then(|v| v.to_str().ok().map(|s| s.to_owned()))
            .or_else(|| request.remote_addr().map(|addr| addr.to_string()))
            .unwrap_or_else(|| String::from("?"));

        let (client_metric_ids, metrics): (Vec<u64>, Vec<Metric>) = request
            .into_inner()
            .definitions
            .into_iter()
            .map(|m| {
                let value_type = protocol::MeasurementValueType::try_from(m.r#type).unwrap().into();
                let metric = Metric {
                    name: m.name,
                    description: m.description,
                    value_type,
                    unit: m.unit.unwrap().try_into().unwrap(),
                };
                (m.id_for_agent, metric)
            })
            .unzip();

        let server_metric_ids = self
            .late_reg
            .lock()
            .await
            .create_metrics_infallible(metrics, client_name)
            .await
            .unwrap();

        let mappings = client_metric_ids
            .into_iter()
            .zip(server_metric_ids)
            .map(|(client_id, server_id)| IdMapping {
                id_for_agent: client_id,
                id_for_collector: server_id.as_u64(),
            })
            .collect();
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
