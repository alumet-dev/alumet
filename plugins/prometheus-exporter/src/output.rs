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
    sync::{Arc, atomic::AtomicU64},
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
    add_attributes_to_labels: bool,
    prefix: String,
    suffix: String,
    pub addr: SocketAddr,
}

impl PrometheusOutput {
    pub fn new(
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
            let metric_name = sanitize_name(format!("{}{}{}", self.prefix, full_metric.name.clone(), self.suffix));

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
                let unit_string = get_unit_string(full_metric);
                let family = Family::<Vec<(String, String)>, Gauge<f64, AtomicU64>>::default();

                if unit_string.is_empty() {
                    registry.register_with_unit(
                        metric_name.clone(),
                        &metric.description,
                        prometheus_client::registry::Unit::Other(sanitize_name(unit_string)),
                        family.clone(),
                    );
                } else {
                    registry.register(metric_name.clone(), &metric.description, family.clone());
                }

                metrics.insert(metric_name.clone(), family.clone());
                // Check that it was correctly registered
                metrics
                    .get(&metric_name)
                    .ok_or_else(|| WriteError::Fatal(anyhow::anyhow!("Failed to retrieve metric after registration")))?
            };

            // Update metric value
            let gauge = family.get_or_create(&labels);
            gauge.set(m.value.as_f64());
        }

        Ok(())
    }
}

/// Helper function to ensure metric/label names follow Prometheus
/// [naming rules](https://prometheus.io/docs/concepts/data_model/#metric-names-and-labels).
fn sanitize_name(name: String) -> String {
    name.chars()
        .enumerate()
        .map(|(i, c)| {
            if i == 0 {
                if c.is_ascii_alphabetic() { c } else { '_' }
            } else if c.is_ascii_alphanumeric() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Helper function that returns the metric's unit according to the Prometheus
/// [base units](https://prometheus.io/docs/practices/naming/#base-units) documentation.
fn get_unit_string(full_metric: &Metric) -> String {
    let unit = match &full_metric.unit.base_unit {
        alumet::units::Unit::Ampere => "amperes",
        alumet::units::Unit::Byte => "bytes",
        alumet::units::Unit::Unity => "",
        alumet::units::Unit::Second => "seconds",
        alumet::units::Unit::Watt => "watts",
        alumet::units::Unit::Joule => "joules",
        alumet::units::Unit::Volt => "volts",
        alumet::units::Unit::Hertz => "hertz",
        alumet::units::Unit::DegreeCelsius => "celsius",
        alumet::units::Unit::DegreeFahrenheit => "fahrenheit",
        alumet::units::Unit::WattHour => "watt_hours",
        alumet::units::Unit::Percent => "ratio",
        alumet::units::Unit::Custom {
            unique_name,
            display_name: _,
        } => unique_name,
    };
    format!("{}{unit}", full_metric.unit.prefix.unique_name())
}

#[cfg(test)]
mod tests {
    use alumet::{
        measurement::WrappedMeasurementType,
        metrics::Metric,
        units::{PrefixedUnit, Unit, UnitPrefix},
    };

    use crate::output::{get_unit_string, sanitize_name};

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("".to_string()), "".to_string());
        assert_eq!(sanitize_name("abc".to_string()), "abc".to_string());
        assert_eq!(sanitize_name("123avc".to_string()), "_23avc".to_string());
        assert_eq!(sanitize_name("cpu_percent_%".to_string()), "cpu_percent__".to_string());
    }

    #[test]
    fn test_get_unit_string() {
        fn new_metric(unit: Unit, prefix: UnitPrefix) -> Metric {
            Metric {
                name: "".to_string(),
                description: "".to_string(),
                value_type: WrappedMeasurementType::F64,
                unit: PrefixedUnit {
                    base_unit: unit,
                    prefix: prefix,
                },
            }
        }

        assert_eq!(
            get_unit_string(&new_metric(Unit::Percent, UnitPrefix::Plain)),
            "ratio".to_string()
        );
        assert_eq!(
            get_unit_string(&new_metric(Unit::Unity, UnitPrefix::Plain)),
            "".to_string()
        );
        assert_eq!(
            get_unit_string(&new_metric(Unit::Byte, UnitPrefix::Kilo)),
            "kilobytes".to_string()
        );
        assert_eq!(
            get_unit_string(&new_metric(Unit::WattHour, UnitPrefix::Nano)),
            "nanowatt_hours".to_string()
        );
    }
}
