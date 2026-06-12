use alumet::{
    measurement::{MeasurementBuffer, WrappedMeasurementValue},
    metrics::Metric,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;
use opentelemetry_proto::tonic::{
    collector::metrics::v1::{ExportMetricsServiceRequest, metrics_service_client::MetricsServiceClient},
    common::v1::{AnyValue, InstrumentationScope, KeyValue, any_value},
    metrics::v1::{
        Gauge, Metric as OtelMetric, NumberDataPoint, ResourceMetrics, ScopeMetrics, metric, number_data_point::Value,
    },
    resource::v1::Resource,
};
use std::{collections::HashMap, env};

use tonic::transport::Channel;

#[derive(Clone)]
pub struct OpenTelemetryOutput {
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
    prefix: String,
    suffix: String,
    collector_host: String,
    // Map where the key is the full metric name (including prefix/suffix)
    metric_map: HashMap<String, OtelMetric>,
    client: Option<MetricsServiceClient<Channel>>,
}

impl OpenTelemetryOutput {
    pub fn new(
        use_unit_display_name: bool,
        add_attributes_to_labels: bool,
        prefix: String,
        suffix: String,
        collector_host: String,
    ) -> anyhow::Result<OpenTelemetryOutput> {
        Ok(Self {
            use_unit_display_name,
            add_attributes_to_labels,
            prefix,
            suffix,
            collector_host,
            metric_map: HashMap::new(),
            client: None,
        })
    }

    fn get_or_init_client(&mut self) -> anyhow::Result<&mut MetricsServiceClient<Channel>> {
        if self.client.is_none() {
            let channel = Channel::from_shared(self.collector_host.clone())
                .context("Invalid collector host URI")?
                .connect_lazy();
            self.client = Some(MetricsServiceClient::new(channel));
        }
        Ok(self.client.as_mut().unwrap())
    }
}

impl alumet::pipeline::Output for OpenTelemetryOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if measurements.is_empty() {
            return Ok(());
        }

        self.metric_map.clear();

        for m in measurements {
            let full_metric = ctx
                .metrics
                .by_id(&m.metric)
                .with_context(|| format!("Unknown metric {:?}", m.metric))
                .map_err(WriteError::from)?;

            let metric_name = format!("{}{}{}", self.prefix, full_metric.name, self.suffix);

            // Prepare Attributes for this specific data point
            let mut attributes = vec![
                make_kv("resource_kind", m.resource.kind().to_string()),
                make_kv("resource_id", m.resource.id_string().unwrap_or("empty".to_string())),
                make_kv("resource_consumer_kind", m.consumer.kind().to_string()),
                make_kv(
                    "resource_consumer_id",
                    m.consumer.id_string().unwrap_or("empty".to_string()),
                ),
            ];

            if self.add_attributes_to_labels {
                for (key, value) in m.attributes() {
                    let v = value.to_string();
                    let v = if v.is_empty() { "empty".to_string() } else { v };
                    attributes.push(make_kv(key, v));
                }
            }

            attributes.sort_by(|a, b| a.key.cmp(&b.key));

            let time_unix_nano = m
                .timestamp
                .duration_since(std::time::UNIX_EPOCH.into())
                .map_err(|e| anyhow::anyhow!("invalid timestamp: {e}"))?
                .as_nanos() as u64;

            let data_point = NumberDataPoint {
                attributes,
                start_time_unix_nano: 0,
                time_unix_nano,
                value: Some(Value::AsDouble(m.value.as_f64())),
                ..Default::default()
            };

            // Lookup for the metric_name entry in the map, or create one if it doesn't exist
            let entry = self
                .metric_map
                .entry(metric_name.clone())
                .or_insert_with(|| OtelMetric {
                    name: metric_name,
                    description: full_metric.description.to_string(),
                    unit: get_unit_string(full_metric, self.use_unit_display_name),
                    data: Some(metric::Data::Gauge(Gauge {
                        data_points: Vec::new(),
                    })),
                    ..Default::default()
                });

            // Push the data point to the existing (or new) OtelMetric
            if let Some(metric::Data::Gauge(ref mut gauge)) = entry.data {
                gauge.data_points.push(data_point);
            }
        }

        // Convert the Map values into a Vector
        let otel_metrics: Vec<OtelMetric> = self.metric_map.values().cloned().collect();

        // Build and Send Request
        let scope = InstrumentationScope {
            name: "alumet".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            attributes: vec![make_kv("tool", "alumet")],
            ..Default::default()
        };

        let resource = Resource {
            attributes: vec![make_kv("service.name", "alumet-otlp-grpc")],
            ..Default::default()
        };

        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: Some(resource),
                scope_metrics: vec![ScopeMetrics {
                    scope: Some(scope),
                    metrics: otel_metrics,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };

        let client = self.get_or_init_client().map_err(WriteError::from)?;

        tokio::runtime::Handle::current()
            .block_on(client.export(request))
            .with_context(|| "Failed to export metrics via gRPC")
            .map_err(WriteError::from)?;

        Ok(())
    }
}

fn make_kv(key: impl Into<String>, value: impl Into<String>) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: Some(AnyValue {
            value: Some(any_value::Value::StringValue(value.into())),
        }),
    }
}

fn get_unit_string(full_metric: &Metric, use_unit_display_name: bool) -> String {
    if use_unit_display_name {
        full_metric.unit.display_name()
    } else {
        full_metric.unit.unique_name()
    }
}
