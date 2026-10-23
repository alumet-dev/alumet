//! InfiniBand per-port packet and byte rate plugin.
//!
//! The plugin polls `/sys/class/infiniband/<device>/ports/<port>` and emits
//! rates computed from the cumulative counters exposed by sysfs. It is
//! Linux-only (sysfs has no equivalent on other targets) and the agent only
//! registers it for `target_os = "linux"`.

use std::{
    collections::{HashMap, HashSet},
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
            // Simple UCUM unit, with the per-second rate carried by the name.
            // A custom unit such as "By/s" would not survive a relay: the relay
            // protocol serializes a unit as a (base unit, prefix) pair and
            // re-parses the base unit name with `Unit::from_str`, which accepts
            // built-in UCUM units only.
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

        // Detection is a startup gate: a node without InfiniBand must fail here,
        // with a clear error, instead of polling sysfs forever.
        let ports = detect_ports(&self.config.root_path, &filters)?;
        log::debug!("{} InfiniBand port(s) detected at startup: {ports:?}", ports.len());

        let metrics = Metrics::new(alumet)?;
        let source = InfinibandSource::new(self.config.root_path.clone(), filters, metrics, ports);

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

/// Detects the ports that the source starts with, and rejects a host without
/// InfiniBand.
///
/// The source re-detects the port list on every poll, so this is only about
/// failing fast at startup: ports that appear later are picked up at runtime,
/// but a missing or unreadable sysfs hierarchy, or an empty port list, means
/// that the plugin cannot ever produce a measurement.
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
    filters: Filters,
    metrics: Metrics,
    /// Ports detected at startup, refreshed on every successful discovery.
    ports: Vec<PortKey>,
    /// Last sample of each port, keyed by port to keep the device and the port
    /// number together.
    previous: HashMap<PortKey, Sample>,
}

impl InfinibandSource {
    pub(crate) fn new(root: PathBuf, filters: Filters, metrics: Metrics, ports: Vec<PortKey>) -> Self {
        Self {
            root,
            filters,
            metrics,
            ports,
            previous: HashMap::new(),
        }
    }
}

impl Source for InfinibandSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        // Refresh the port list to follow hot-plug and hot-unplug. If the whole
        // hierarchy becomes unreadable, keep the known ports and their baseline:
        // a transient sysfs failure must neither stop the source nor reset the
        // counters.
        match discover_ports(&self.root, &self.filters) {
            Ok(ports) => self.ports = ports,
            Err(e) => log::debug!(
                "cannot refresh the InfiniBand port list ({e:#}); keeping {} known port(s)",
                self.ports.len()
            ),
        }

        for key in &self.ports {
            let counters = match read_counters(&self.root, key) {
                Ok(counters) => counters,
                Err(e) => {
                    // The port may be disappearing: keep its last sample, the next
                    // successful read either continues the series or re-baselines.
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

        // Forget ports that disappeared so that a future re-appearance starts
        // from a fresh baseline.
        let known: HashSet<&PortKey> = self.ports.iter().collect();
        self.previous.retain(|key, _| known.contains(&key));
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

    let push = |acc: &mut MeasurementAccumulator, metric, value, direction| {
        acc.push(
            MeasurementPoint::new(timestamp, metric, resource.clone(), consumer.clone(), value)
                .with_attr("interface", key.device.clone())
                .with_attr("port", key.port as u64)
                .with_attr("direction", direction),
        );
    };

    push(acc, metrics.packets, rates.rcv_packets_per_s, "rx");
    push(acc, metrics.packets, rates.xmit_packets_per_s, "tx");
    push(acc, metrics.bytes, rates.rcv_bytes_per_s, "rx");
    push(acc, metrics.bytes, rates.xmit_bytes_per_s, "tx");
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
    fn metric_units_survive_relay_serialization() {
        // The relay protocol serializes a unit as a (base unit, prefix) pair and
        // re-parses the base unit name with `Unit::from_str`, which only accepts
        // the built-in UCUM units. A custom unit is rejected because it is
        // custom, not because of the characters it contains: "By/s" would fail
        // just like any other custom name.
        for unit in [Unit::Hertz, Unit::Byte] {
            let round_trip = unit
                .unique_name()
                .parse::<Unit>()
                .expect("built-in units are known to Unit::from_str");
            assert_eq!(round_trip, unit);
        }

        let per_second = Unit::Custom {
            unique_name: "By/s".to_string(),
            display_name: "B/s".to_string(),
        };
        assert!(
            per_second.unique_name().parse::<Unit>().is_err(),
            "custom units cannot cross the relay, which is why this plugin uses built-in units"
        );
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
