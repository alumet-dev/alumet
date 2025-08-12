use alumet::{
    measurement::{MeasurementBuffer, WrappedMeasurementValue},
    metrics::Metric,
    pipeline::elements::{error::WriteError, output::OutputContext},
};
use anyhow::Context;
use prometheus_client::{
    metrics::{family::Family, gauge::Gauge},
    registry::Registry,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::AtomicU64, Arc},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct MetricState {
    pub registry: Arc<RwLock<Registry>>,
    metrics: Arc<RwLock<HashMap<String, Family<Vec<(String, String)>, Gauge<f64, AtomicU64>>>>>,
}

#[derive(Clone)]
pub struct PrometheusOutput {
    pub state: MetricState,
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
    prefix: String,
    suffix: String,
    pub addr: SocketAddr,
}

impl PrometheusOutput {
    pub fn new(
        use_unit_display_name: bool,
        add_attributes_to_labels: bool,
        port: u16,
        host: String,
        prefix: String,
        suffix: String,
    ) -> anyhow::Result<PrometheusOutput> {
        // Create metric state
        let registry = Arc::new(RwLock::new(Registry::default()));
        let metrics = Arc::new(RwLock::new(HashMap::new()));
        let state = MetricState { registry, metrics };

        // Configure the HTTP server to expose the metrics
        let addr: SocketAddr = format!("{}:{}", host, port)
            .parse()
            .context("Invalid host:port configuration")?;

        Ok(Self {
            state,
            use_unit_display_name,
            add_attributes_to_labels,
            prefix,
            suffix,
            addr,
        })
    }
}

impl alumet::pipeline::Output for PrometheusOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        if measurements.is_empty() {
            return Ok(());
        }

        // Ensure threads reading and writing are handled correctly
        let mut metrics = self.state.metrics.blocking_write();
        let mut registry = self.state.registry.blocking_write();

        for m in measurements {
            let metric = ctx.metrics.by_id(&m.metric).unwrap();

            // Configure the name of the metric
            let full_metric = ctx
                .metrics
                .by_id(&m.metric)
                .with_context(|| format!("Unknown metric {:?}", m.metric))?;
            let metric_name = format!(
                "{}{}{}",
                self.prefix,
                sanitize_name(full_metric.name.clone()),
                self.suffix
            );

            // Create the default labels for all metrics and optionally add attributes
            let mut labels = vec![
                ("resource_kind".to_string(), m.resource.kind().to_string()),
                ("resource_id".to_string(), m.resource.id_string().unwrap_or_default()),
                ("resource_consumer_kind".to_string(), m.consumer.kind().to_string()),
                (
                    "resource_consumer_id".to_string(),
                    m.consumer.id_string().unwrap_or_default(),
                ),
            ];
            if self.add_attributes_to_labels {
                // Add attributes as labels
                for (key, value) in m.attributes() {
                    let key = sanitize_name(key.to_owned());
                    labels.push((key, value.to_string()));
                }
            }
            labels.sort_by(|a, b| a.0.cmp(&b.0));

            // Each family vector contains a metric with all associated metrics and differentiated by the labels
            let family = if let Some(family) = metrics.get(&metric_name) {
                family
            } else {
                let unit_string = get_unit_string(full_metric, self.use_unit_display_name);
                let family = Family::<Vec<(String, String)>, Gauge<f64, AtomicU64>>::default();
                registry.register_with_unit(
                    metric_name.clone(),
                    &metric.description,
                    prometheus_client::registry::Unit::Other(unit_string),
                    family.clone(),
                );

                metrics.insert(metric_name.clone(), family.clone());
                // Check that it was correctly registered
                metrics
                    .get(&metric_name)
                    .ok_or_else(|| WriteError::Fatal(anyhow::anyhow!("Failed to retrieve metric after registration")))?
            };

            // Update metric value
            let gauge = family.get_or_create(&labels);
            match m.value {
                WrappedMeasurementValue::F64(v) => gauge.set(v as f64),
                WrappedMeasurementValue::U64(v) => gauge.set(v as f64),
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

fn get_unit_string(full_metric: &Metric, use_unit_display_name: bool) -> String {
    if use_unit_display_name {
        full_metric.unit.display_name()
    } else {
        full_metric.unit.unique_name()
    }
}
