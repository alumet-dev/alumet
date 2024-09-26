use std::time::{Duration, UNIX_EPOCH};

use alumet::{
    measurement::{
        AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType, WrappedMeasurementValue,
    },
    metrics::{Metric, RawMetricId},
    pipeline::registry,
    resources::{InvalidConsumerError, InvalidResourceError, Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use tonic::Status;

use crate::protocol::{
    self,
    metric_collector_server::{MetricCollector, MetricCollectorServer},
    register_reply::IdMapping,
    Empty, RegisterReply,
};

pub struct MetricCollectorImpl {
    out_tx: tokio::sync::mpsc::Sender<MeasurementBuffer>,
    metrics_tx: registry::MetricSender,
}

impl MetricCollectorImpl {
    pub fn new(out_tx: tokio::sync::mpsc::Sender<MeasurementBuffer>, metrics_tx: registry::MetricSender) -> Self {
        Self { out_tx, metrics_tx }
    }

    pub fn into_service(self) -> MetricCollectorServer<Self> {
        MetricCollectorServer::new(self)
    }
}

#[tonic::async_trait]
impl MetricCollector for MetricCollectorImpl {
    /// Handles a gRPC request to ingest new measurement points.
    async fn ingest_measurements(
        &self,
        request: tonic::Request<tonic::Streaming<protocol::MeasurementBuffer>>,
    ) -> Result<tonic::Response<Empty>, Status> {
        // Transforms gRPC structures into ALUMET measurement points.
        fn convert_protobuf_to_alumet(m: protocol::MeasurementPoint) -> anyhow::Result<MeasurementPoint> {
            fn convert_attribute(attr: protocol::MeasurementAttribute) -> anyhow::Result<(String, AttributeValue)> {
                let value = attr
                    .value
                    .with_context(|| format!("missing attribute value for key {}", attr.key))?;
                Ok((attr.key, value.into()))
            }

            let timestamp = Timestamp::from(UNIX_EPOCH + Duration::new(m.timestamp_secs, m.timestamp_nanos));
            let value = m.value.context("missing value")?.into();
            let resource = Resource::try_from(m.resource.context("missing resource")?).unwrap();
            let consumer = ResourceConsumer::try_from(m.consumer.context("missing resource consumer")?).unwrap();
            let attributes: anyhow::Result<Vec<_>> = m.attributes.into_iter().map(convert_attribute).collect();
            let metric = RawMetricId::from_u64(m.metric);
            Ok(MeasurementPoint::new_untyped(timestamp, metric, resource, consumer, value).with_attr_vec(attributes?))
        }

        // Handle each message as it arrives
        let mut streaming: tonic::Streaming<protocol::MeasurementBuffer> = request.into_inner();
        loop {
            let incoming = streaming.message().await;
            match incoming {
                Ok(Some(buf)) => {
                    // convert protobuf structs to Alumet structs
                    let converted: anyhow::Result<Vec<MeasurementPoint>> =
                        buf.points.into_iter().map(convert_protobuf_to_alumet).collect();
                    let converted = converted.map_err(|e| Status::invalid_argument(e.to_string()))?;
                    // send the measurements to the rest of the pipeline
                    self.out_tx
                        .send(MeasurementBuffer::from(converted))
                        .await
                        .expect("out_tx receiving end should not be dropped while an input is still running");
                }
                Ok(None) => {
                    // end of the stream, the client will no longer send messages
                    break;
                }
                Err(status) => {
                    let client_name = status
                        .metadata()
                        .get(crate::CLIENT_NAME_HEADER)
                        .and_then(|name| name.to_str().ok())
                        .unwrap_or("?");
                    log::error!("Error received from the relay client '{client_name}': {status}");
                }
            }
        }
        // Done.
        Ok(tonic::Response::new(Empty {}))
    }

    /// Handles a gRPC request to register new metrics.
    async fn register_metrics(
        &self,
        request: tonic::Request<crate::protocol::MetricDefinitions>,
    ) -> Result<tonic::Response<RegisterReply>, Status> {
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
        Ok(tonic::Response::new(RegisterReply { mappings }))
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
