use alumet::{
    measurement::{MeasurementBuffer, WrappedMeasurementValue},
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;
use opentelemetry::{global, InstrumentationScope, KeyValue};
use opentelemetry_otlp::{MetricExporter, WithExportConfig};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use std::{env, sync::OnceLock};

#[derive(Clone)]
pub struct OpenTelemetryOutput {
    append_unit_to_metric_name: bool,
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
    prefix: String,
    suffix: String,
    initialized: bool,
    collector_host: String,
}



impl OpenTelemetryOutput {
    pub fn new(
        append_unit_to_metric_name: bool,
        use_unit_display_name: bool,
        add_attributes_to_labels: bool,
        prefix: String,
        suffix: String,
        collector_host: String,
    ) -> anyhow::Result<OpenTelemetryOutput> {
        Ok(Self {
            append_unit_to_metric_name,
            use_unit_display_name,
            add_attributes_to_labels,
            prefix,
            suffix,
            initialized: false,
            collector_host: format!("{}{}", collector_host, "/v1/metrics")
        })
    }
    pub fn initialize(&mut self) {
        // Needs to be created inside the tokio thread
        let meter_provider = self.init_metrics();
        global::set_meter_provider(meter_provider.clone());
    }
    fn get_resource(&mut self) -> Resource {
        static RESOURCE: OnceLock<Resource> = OnceLock::new();
        RESOURCE
            .get_or_init(|| Resource::builder().with_service_name("alumet-otlp-grpc").build())
            .clone()
    }
    
    fn init_metrics(&mut self) -> SdkMeterProvider {
        let exporter = MetricExporter::builder()
            .with_tonic()
            .with_endpoint(self.collector_host.clone())
            .build()
            .expect("Failed to create metric exporter");
    
        SdkMeterProvider::builder()
            .with_periodic_exporter(exporter)
            .with_resource(self.get_resource())
            .build()
    }
}

impl alumet::pipeline::Output for OpenTelemetryOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if measurements.is_empty() {
            return Ok(());
        }
        if !self.initialized {
            self.initialize();
            self.initialized = true;
        }
        let common_scope_attributes = vec![KeyValue::new("tool", "alumet")];
        let scope = InstrumentationScope::builder("alumet")
            .with_version(env!("CARGO_PKG_VERSION"))
            .with_attributes(common_scope_attributes)
            .build();
        for m in measurements {
            let metric = ctx.metrics.by_id(&m.metric).unwrap().clone();
            // Configure the name of the metric
            let full_metric = ctx
                .metrics
                .by_id(&m.metric)
                .with_context(|| format!("Unknown metric {:?}", m.metric))?;
            let metric_name = format!(
                "{}{}{}",
                self.prefix,
                sanitize_name(if self.append_unit_to_metric_name {
                    let unit_string = if self.use_unit_display_name {
                        full_metric.unit.display_name()
                    } else {
                        full_metric.unit.unique_name()
                    };
                    if unit_string.is_empty() {
                        full_metric.name.to_owned()
                    } else {
                        format!("{}_{}", full_metric.name, unit_string)
                    }
                } else {
                    full_metric.name.clone()
                }),
                self.suffix
            );

            // Create the default labels for all metrics and optionally add attributes
            let mut labels = vec![
                KeyValue::new("resource_kind".to_string(), m.resource.kind().to_string()),
                KeyValue::new("resource_id".to_string(), m.resource.id_string().unwrap_or_default()),
                KeyValue::new("resource_consumer_kind".to_string(), m.consumer.kind().to_string()),
                KeyValue::new(
                    "resource_consumer_id".to_string(),
                    m.consumer.id_string().unwrap_or_default(),
                ),
            ];
            if self.add_attributes_to_labels {
                // Add attributes as labels
                for (key, value) in m.attributes() {
                    let key = sanitize_name(key.to_owned());
                    labels.push(KeyValue::new(key, value.to_string()));
                }
            }
            // OpenTelemetry does not accept empty label
            for label in &mut labels {
                if label.value == "".into() {
                    label.value = "empty".to_string().into();
                }
            }
            labels.sort_by(|a, b| a.key.cmp(&b.key));

            // Prepare the meter provider
            let meter = global::meter_with_scope(scope.clone());
            let gauge = meter
                .f64_gauge(metric_name)
                .with_description(metric.description.to_string())
                .with_unit(metric.unit.display_name())
                .build();
            match m.value {
                WrappedMeasurementValue::F64(v) => gauge.record(v as f64, &labels),
                WrappedMeasurementValue::U64(v) => gauge.record(v as f64, &labels),
            };
        }

        Ok(())
    }
}

// Helper functions to ensure metric/label names follow Prometheus naming rules
fn sanitize_name(name: String) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}
