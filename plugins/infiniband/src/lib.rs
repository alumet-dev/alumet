//! InfiniBand per-port packet and byte rate plugin.
//!
//! Polls `/sys/class/infiniband/<device>/ports/<port>` counters and emits
//! rates. Linux-only; the agent registers it under `target_os = "linux"`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        Source,
        elements::{error::PollError, source::trigger::TriggerSpec},
    },
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    resources::{Resource, ResourceConsumer},
    units::Unit,
};
use anyhow::{Context, anyhow};
use rate::{Rates, Sample, compute_rates};
use serde::{Deserialize, Serialize};
use sysfs::{Filters, PortKey, discover_ports, read_counters};

pub mod rate;
pub mod sysfs;

use sysfs::DEFAULT_ROOT;

/// Configuration of the InfiniBand plugin.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Interval between two polls.
    #[serde(with = "humantime_serde", default = "default_poll_interval")]
    pub poll_interval: Duration,

    /// Root of the InfiniBand sysfs hierarchy.
    #[serde(default = "default_root_path")]
    pub root_path: PathBuf,

    /// Devices to monitor. An empty list means all devices.
    #[serde(default)]
    pub devices: Vec<String>,

    /// Ports to monitor. An empty list means all ports.
    #[serde(default)]
    pub ports: Vec<u32>,
}

fn default_poll_interval() -> Duration {
    Duration::from_secs(5)
}

fn default_root_path() -> PathBuf {
    PathBuf::from(DEFAULT_ROOT)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: default_poll_interval(),
            root_path: default_root_path(),
            devices: Vec::new(),
            ports: Vec::new(),
        }
    }
}

/// Metrics emitted by the InfiniBand plugin.
pub(crate) struct Metrics {
    pub(crate) packets: TypedMetricId<f64>,
    pub(crate) bytes: TypedMetricId<f64>,
}

impl Metrics {
    fn new(alumet: &mut AlumetPluginStart) -> anyhow::Result<Self> {
        let packets = alumet.create_metric::<f64>(
            "infiniband_packets_per_second",
            Unit::Hertz,
            "InfiniBand packet rate per port and direction",
        )?;
        let bytes = alumet.create_metric::<f64>(
            "infiniband_bytes_per_second",
            // Per-second rate carried by the name: the relay only accepts
            // built-in UCUM units (see README, "Sysfs counters").
            Unit::Byte,
            "InfiniBand byte rate per port and direction",
        )?;
        Ok(Self { packets, bytes })
    }
}

pub struct InfinibandPlugin {
    config: Config,
}

impl AlumetPlugin for InfinibandPlugin {
    fn name() -> &'static str {
        "infiniband"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(InfinibandPlugin { config }))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let filters = Filters {
            devices: self.config.devices.clone(),
            ports: self.config.ports.clone(),
        };

        // Fail fast on a node without InfiniBand instead of polling forever.
        let ports = detect_ports(&self.config.root_path, &filters)?;
        log::debug!("{} InfiniBand port(s) detected at startup: {ports:?}", ports.len());

        let metrics = Metrics::new(alumet)?;
        let source = InfinibandSource::new(self.config.root_path.clone(), metrics, ports);

        alumet.add_source(
            "ports",
            Box::new(source),
            TriggerSpec::at_interval(self.config.poll_interval),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Detects the ports the source starts with; fails if there is none.
fn detect_ports(root: &Path, filters: &Filters) -> anyhow::Result<Vec<PortKey>> {
    let ports = discover_ports(root, filters).with_context(|| {
        format!(
            "cannot detect InfiniBand ports under {} (is the InfiniBand stack available?)",
            root.display()
        )
    })?;
    if ports.is_empty() {
        return Err(anyhow!(
            "no InfiniBand port found under {} (devices={:?}, ports={:?})",
            root.display(),
            filters.devices,
            filters.ports
        ));
    }
    Ok(ports)
}

/// Managed source that polls the InfiniBand sysfs counters.
pub(crate) struct InfinibandSource {
    root: PathBuf,
    metrics: Metrics,
    /// Ports detected at startup
    ports: Vec<PortKey>,
    /// Last sample of each port, keyed by port to keep device and port together.
    previous: HashMap<PortKey, Sample>,
}

impl InfinibandSource {
    pub(crate) fn new(root: PathBuf, metrics: Metrics, ports: Vec<PortKey>) -> Self {
        Self {
            root,
            metrics,
            ports,
            previous: HashMap::new(),
        }
    }
}

impl Source for InfinibandSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for key in &self.ports {
            let counters = match read_counters(&self.root, key) {
                Ok(counters) => counters,
                Err(e) => {
                    // Keep the last sample: the next successful read either
                    // continues the series or re-baselines.
                    log::debug!("failed to read InfiniBand counters for {key}: {e:#}");
                    continue;
                }
            };

            let current = Sample { timestamp, counters };
            if let Some(previous) = self.previous.get(key) {
                match compute_rates(previous, &current) {
                    Some(rates) => push_points(acc, &self.metrics, key, timestamp, &rates),
                    None => log::debug!(
                        "cannot compute rates for {key}: counter reset, wrap-around, or non-positive elapsed time; resetting baseline"
                    ),
                }
            }

            self.previous.insert(key.clone(), current);
        }

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.previous.clear();
        Ok(())
    }
}

/// Adds the four measurement points of one port to `acc`.
fn push_points(
    acc: &mut MeasurementAccumulator,
    metrics: &Metrics,
    key: &PortKey,
    timestamp: Timestamp,
    rates: &Rates,
) {
    let resource = Resource::custom("infiniband_port", key.to_string());
    let consumer = ResourceConsumer::LocalMachine;

    let mut push = |metric, value, direction| {
        acc.push(
            MeasurementPoint::new(timestamp, metric, resource.clone(), consumer.clone(), value)
                .with_attr("interface", key.device.clone())
                .with_attr("port", key.port as u64)
                .with_attr("direction", direction),
        );
    };

    push(metrics.packets, rates.rcv_packets_per_s, "rx");
    push(metrics.packets, rates.xmit_packets_per_s, "tx");
    push(metrics.bytes, rates.rcv_bytes_per_s, "rx");
    push(metrics.bytes, rates.xmit_bytes_per_s, "tx");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_poll_interval_and_filters() {
        let text = r#"
            poll_interval = "1s"
            root_path = "/tmp/infiniband-test"
            devices = ["mlx5_0", "mlx5_1"]
            ports = [1, 2]
        "#;
        let config: Config = toml::from_str(text).unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.root_path, PathBuf::from("/tmp/infiniband-test"));
        assert_eq!(config.devices, vec!["mlx5_0", "mlx5_1"]);
        assert_eq!(config.ports, vec![1, 2]);
    }

    #[test]
    fn defaults_are_applied() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.root_path, PathBuf::from(DEFAULT_ROOT));
        assert!(config.devices.is_empty());
        assert!(config.ports.is_empty());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let text = r#"
            poll_interval = "1s"
            poll_intervall = "2s"
        "#;
        let result: Result<Config, _> = toml::from_str(text);
        assert!(result.is_err(), "deny_unknown_fields should reject typos");
    }
}
