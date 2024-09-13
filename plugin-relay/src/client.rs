use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::protocol::metric_collector_client::MetricCollectorClient;
use crate::protocol::{self, RegisterReply};

use alumet::measurement::{
    AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementType, WrappedMeasurementValue,
};
use alumet::metrics::{Metric, RawMetricId};
use alumet::pipeline::builder::elements::{MetricListenerRegistration, OutputRegistration};
use alumet::pipeline::elements::error::WriteError;
use alumet::pipeline::elements::output::OutputContext;
use alumet::pipeline::registry::MetricListener;
use alumet::plugin::rust::{deserialize_config, serialize_config, AlumetPlugin};
use alumet::plugin::{AlumetPluginStart, AlumetPreStart, ConfigTable};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tonic::transport::Channel;

pub struct RelayClientPlugin {
    client_name: String,
    collector_uri: String,
    metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
}

#[derive(Serialize, Deserialize)]
struct Config {
    /// The name that this client will use to identify itself to the collector server.
    /// Defaults to the hostname.
    #[serde(default = "default_client_name")]
    client_name: String,

    /// The URI of the collector, for instance `http://127.0.0.1:50051`.
    #[serde(default = "default_collector_uri")]
    collector_uri: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_name: default_client_name(),
            collector_uri: default_collector_uri(),
        }
    }
}

fn default_client_name() -> String {
    hostname::get()
        .expect("No client_name specified and unable to retrieve the hostname of the current node.")
        .to_string_lossy()
        .to_string()
}

fn default_collector_uri() -> String {
    String::from("http://[::1]:50051")
}

impl AlumetPlugin for RelayClientPlugin {
    fn name() -> &'static str {
        "relay-client"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // Read the configuration.
        let config = deserialize_config::<Config>(config)?;

        // Initialize a thread-safe HashMap to store the mapping 'local metric id' -> 'collector metric id'
        let metric_ids = Arc::new(Mutex::new(HashMap::new()));

        // Return initialized plugin.
        Ok(Box::new(Self {
            client_name: config.client_name,
            collector_uri: config.collector_uri,
            metric_ids,
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let collector_uri = self.collector_uri.clone();
        let client_name = self.client_name.clone();
        let metric_ids = self.metric_ids.clone();

        // The output cannot be created right now: we need the tokio Runtime (see below).
        alumet.add_output_builder(move |ctx| {
            log::info!("Connecting to gRPC server {collector_uri}...");

            // Connect to gRPC server, using the tokio runtime in which Alumet will trigger the output.
            // We also store the runtime handle, for use in `pre_pipeline_start`.
            // This is important because a Tonic gRPC client can only be used from the runtime that it has been initialized with.
            let rt = ctx.async_runtime();
            let client = rt
                .block_on(RelayClient::new(collector_uri, client_name.clone(), metric_ids))
                .context("gRPC connection error")?;

            log::info!("Successfully connected with client name {client_name}.");
            let output = Box::new(RelayOutput { client });
            Ok(OutputRegistration {
                name: ctx.output_name("grpc"),
                output,
            })
        });
        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut AlumetPreStart) -> anyhow::Result<()> {
        let collector_uri = self.collector_uri.clone();
        let client_name = self.client_name.clone();
        let metric_ids = self.metric_ids.clone();

        // Clone the existing metrics (which have been registered by the `start` methods of all the plugins).
        let existing_metrics = alumet
            .metrics()
            .iter()
            .map(|(id, def)| (id.clone(), def.clone()))
            .collect();

        // Get notified of late metric registration. (TODO: is this the best way? Would it be faster to inspect the points in the output instead?)
        // Also register the existing metrics on the async pipeline.
        alumet.add_metric_listener_builder(move |ctx| {
            let rt = ctx.async_runtime();

            let mut client = rt.block_on(async move {
                // We need to create another client, the one created in `start` has been moved to the output.
                let mut client = RelayClient::new(collector_uri, client_name, metric_ids).await?;

                // Register the existing metrics.
                client.register_metrics(existing_metrics).await?;

                // Pass the client, for use in the listener.
                Ok::<RelayClient, anyhow::Error>(client)
            })?;

            // Build a listener that uses the client.
            let listener: Box<dyn MetricListener> = Box::new(move |new_metrics| {
                // register the metrics, wait for the message to be sent
                let rt = tokio::runtime::Handle::current();
                rt.block_on(client.register_metrics(new_metrics))?;
                Ok(())
            });

            Ok(MetricListenerRegistration {
                name: ctx.listener_name("grpc-registration"),
                listener,
            })
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct RelayOutput {
    client: RelayClient,
}

impl alumet::pipeline::Output for RelayOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, _ctx: &OutputContext) -> Result<(), WriteError> {
        // Acquire the client.
        let client = &mut self.client;
        // Get a handle to the current tokio runtime. This works because Alumet outputs are executed inside of a tokio runtime.
        let handle = tokio::runtime::Handle::current();
        // Execute send_measurements in a blocking way (wait for it to finish).
        handle
            .block_on(client.send_measurements(measurements))
            .context("error in send_measurements")?;
        Ok(())
    }
}

struct RelayClient {
    /// gRPC client
    grpc_client: MetricCollectorClient<Channel>,
    /// Maps local ids to collector ids.
    metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
    /// Name of this client, should be unique (from the collector's pov).
    client_name: String,
}

impl RelayClient {
    /// Creates a new `RelayClient` by connecting to a gRPC endpoint.
    async fn new(
        uri: String,
        client_name: String,
        metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
    ) -> anyhow::Result<RelayClient> {
        let uri_clone = uri.clone();
        let grpc_client = MetricCollectorClient::connect(uri)
            .await
            .with_context(|| format!("could not connect to {uri_clone}"))?;
        let client = RelayClient {
            grpc_client,
            client_name,
            metric_ids,
        };
        Ok(client)
    }

    /// Sends measurements via gRPC.
    async fn send_measurements(&mut self, measurements: &MeasurementBuffer) -> anyhow::Result<()> {
        /// Convert Alumet measurements to the appropriate Protocol Buffer structure for gRPC.
        fn convert_alumet_to_protobuf(
            m: &MeasurementPoint,
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

            // convert value
            let value = m.value.clone().into(); // TODO avoid cloning by making Output::write accept an owned MeasurementBuffer?

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
                    value: Some(attr_value.clone().into()),
                })
                .collect();

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

        let metric_ids = self.metric_ids.lock().unwrap();
        let points: Vec<protocol::MeasurementPoint> = measurements
            .iter()
            .map(|point| convert_alumet_to_protobuf(point, &metric_ids))
            .collect();

        log::debug!("Sending gRPC request with {} measurement points", points.len());
        let request = tonic::Request::new(protocol::MeasurementBuffer { points });
        let response = self.grpc_client.ingest_measurements(request).await;
        log::trace!("RESPONSE={:?}", response);

        response?;
        // TODO add details to the error (in server) and handle them here
        Ok(())
    }

    /// Sends metric registration requests via gRPC.
    async fn register_metrics(&mut self, metrics: Vec<(RawMetricId, Metric)>) -> anyhow::Result<()> {
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
        let mut request = tonic::Request::new(protocol::MetricDefinitions { definitions });

        // Add a header to tell the server who we are.
        request
            .metadata_mut()
            .append("x-alumet-client", self.client_name.parse().unwrap());

        // Wait for the response.
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
