use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use alumet::measurement::{AttributeValue, MeasurementPoint, WrappedMeasurementType, WrappedMeasurementValue};
use alumet::metrics::{Metric, RawMetricId};
use alumet::pipeline::elements::output::AsyncOutputStream;

use anyhow::{anyhow, Context};
use futures::Stream;

use super::AsciiString;

pub struct RelayClient {
    /// gRPC client
    grpc_client: ConnectedMetricCollectorClient,
    /// Maps local ids to collector ids.
    metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
}

type ConnectedMetricCollectorClient = MetricCollectorClient<InterceptedService<Channel, MetadataProvider>>;

struct MetadataProvider {
    /// Name of this client, should be unique (from the collector's pov).
    client_name: AsciiString,
}

impl Interceptor for MetadataProvider {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        req.metadata_mut()
            .append(crate::CLIENT_NAME_HEADER, self.client_name.metadata_value.clone());
        Ok(req)
    }
}

impl RelayClient {
    /// Creates a new `RelayClient` by connecting to a gRPC endpoint.
    pub async fn new(
        uri: String,
        client_name: AsciiString,
        metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
    ) -> anyhow::Result<RelayClient> {
        let uri_clone = uri.clone();
        let conn = tonic::transport::Endpoint::new(uri)
            .with_context(|| format!("invalid uri {uri_clone}"))?
            .connect()
            .await
            .with_context(|| format!("could not connect to {uri_clone}"))?;
        let interceptor = MetadataProvider { client_name };
        let grpc_client = MetricCollectorClient::with_interceptor(conn, interceptor);
        let client = RelayClient {
            grpc_client,
            metric_ids,
        };
        Ok(client)
    }

    pub fn process_measurement_stream(
        self,
        measurements: AsyncOutputStream,
        buffer_size: usize,
        buffer_timeout: Duration,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        fn convert_alumet_to_protobuf(
            m: MeasurementPoint,
            metric_ids: &HashMap<u64, u64>,
        ) -> protocol::MeasurementPoint {
            // convert metric id
            let metric = *metric_ids.get(&m.metric.as_u64()).unwrap();

            // TODO: if the connection drops, the client will retry to connect, which is good.
            // But if the server has crashed, its MetricRegistry has been reinitialized,
            // and the metrics of the client should be registered again (otherwise the server will error on metric ingestion).

            // convert timestamp
            let time_diff = SystemTime::from(m.timestamp)
                .duration_since(UNIX_EPOCH)
                .expect("Every timestamp should be after the UNIX_EPOCH");

            // convert resource and consumer
            let resource = protocol::Resource {
                kind: m.resource.kind().to_owned(),
                id: m.resource.id_string(),
            };
            let consumer = protocol::ResourceConsumer {
                kind: m.consumer.kind().to_owned(),
                id: m.consumer.id_string(),
            };

            // convert attributes
            let attributes = m
                .attributes()
                .map(|(attr_key, attr_value)| protocol::MeasurementAttribute {
                    key: attr_key.to_owned(),
                    value: Some(attr_value.to_owned().into()), // todo avoid clone by using a dedicated structure to expose the attributes
                })
                .collect();

            // convert value
            let value = m.value.into();

            // create point
            protocol::MeasurementPoint {
                metric,
                timestamp_secs: time_diff.as_secs(),
                timestamp_nanos: time_diff.subsec_nanos(),
                value: Some(value),
                resource: Some(resource),
                consumer: Some(consumer),
                attributes,
            }
        }

        use futures::StreamExt;
        let grpc_client = self.grpc_client;
        let metric_ids = self.metric_ids;

        let filtered_stream = measurements
            .0
            .filter_map(move |maybe_buf| async move {
                match maybe_buf {
                    Ok(buf) => Some(futures::stream::iter(buf)),
                    Err(_) => None,
                }
            })
            .flatten();
        // TODO(opt) optimize the inner stream away by creating a special version of
        // `chunks_timeout` that keep the buffers (to avoid copies) but counts the number of points
        let buffered_stream = tokio_stream::StreamExt::chunks_timeout(filtered_stream, buffer_size, buffer_timeout);
        let converted_stream = buffered_stream.map(move |points| {
            let metric_ids = metric_ids.clone();
            let metric_ids = metric_ids.lock().unwrap();
            let points = points
                .into_iter()
                .map(|m| convert_alumet_to_protobuf(m, &metric_ids))
                .collect();
            protocol::MeasurementBuffer { points }
        });

        // This function is necessary for the type inference to work,
        // otherwise the compiler cannot prove that the type of the future returned by the tonic function is correct
        // and produces weird "higher-ranked lifetime error".
        fn ingest<S: Stream<Item = protocol::MeasurementBuffer> + Send + 'static>(
            mut client: ConnectedMetricCollectorClient,
            stream: S,
        ) -> impl Future<Output = anyhow::Result<()>> + 'static {
            async move {
                let res = client.ingest_measurements(stream).await;
                match res {
                    Ok(_response) => Ok(()),
                    Err(status) => Err(anyhow!("error in grpc streaming ingest_measurements: status {status}")),
                }
            }
        }
        // TODO add the client name in the header

        ingest(grpc_client, converted_stream)
    }

    /// Sends metric registration requests via gRPC.
    pub async fn register_metrics(&mut self, metrics: Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> {
        // Convert Alumet metrics to the appropriate Protocol Buffer structures.
        let definitions: Vec<protocol::metric_definitions::MetricDef> = metrics
            .into_iter()
            .map(|(id, metric)| protocol::metric_definitions::MetricDef {
                id_for_agent: id.as_u64(),
                name: metric.name,
                description: metric.description,
                r#type: protocol::MeasurementValueType::from(metric.value_type) as i32,
                unit: Some(protocol::PrefixedUnit {
                    prefix: metric.unit.prefix.unique_name().to_string(),
                    base_unit: metric.unit.base_unit.unique_name().to_string(),
                }),
            })
            .collect();

        // Create the gRPC request.
        let request = tonic::Request::new(protocol::MetricDefinitions { definitions });

        // Send and wait for the response.
        let response = self.grpc_client.register_metrics(request).await;
        log::debug!("RESPONSE={:?}", response);
        let response = response?;

        let reply: RegisterReply = response.into_inner();
        let mut metric_ids = self.metric_ids.lock().unwrap();
        for mapping in reply.mappings {
            metric_ids.insert(mapping.id_for_agent, mapping.id_for_collector);
        }
        Ok(())
    }
}

impl From<WrappedMeasurementType> for protocol::MeasurementValueType {
    fn from(value: WrappedMeasurementType) -> Self {
        match value {
            WrappedMeasurementType::F64 => protocol::MeasurementValueType::F64,
            WrappedMeasurementType::U64 => protocol::MeasurementValueType::U64,
        }
    }
}

impl From<WrappedMeasurementValue> for protocol::measurement_point::Value {
    fn from(value: WrappedMeasurementValue) -> Self {
        match value {
            WrappedMeasurementValue::F64(x) => protocol::measurement_point::Value::F64(x),
            WrappedMeasurementValue::U64(x) => protocol::measurement_point::Value::U64(x),
        }
    }
}

impl From<AttributeValue> for protocol::measurement_attribute::Value {
    fn from(value: AttributeValue) -> Self {
        match value {
            AttributeValue::F64(v) => protocol::measurement_attribute::Value::F64(v),
            AttributeValue::U64(v) => protocol::measurement_attribute::Value::U64(v),
            AttributeValue::Bool(v) => protocol::measurement_attribute::Value::Bool(v),
            AttributeValue::String(v) => protocol::measurement_attribute::Value::Str(v),
            AttributeValue::Str(v) => protocol::measurement_attribute::Value::Str(v.to_owned()),
        }
    }
}
