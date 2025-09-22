mod probe;

use anyhow::Context;
use probe::GraceHopperSource;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

use alumet::{
    metrics::TypedMetricId,
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::{PrefixedUnit, Unit},
};

mod hwmon;

pub struct GraceHopperPlugin {
    config: Config,
}

impl AlumetPlugin for GraceHopperPlugin {
    fn name() -> &'static str {
        "grace-hopper"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(GraceHopperPlugin { config }))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let hwmon_path = PathBuf::from(&self.config.root_path);
        let devices = hwmon::explore(&hwmon_path)
            .context("could not find (or init) hwmon devices, is power telemetry enabled?")?;

        let metrics = Metrics::new(alumet)?;
        let source = GraceHopperSource::new(metrics, devices);

        alumet.add_source(
            "hwmon",
            Box::new(source),
            TriggerSpec::at_interval(self.config.poll_interval),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Initial interval between two measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Path to check hwmon.
    pub root_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1 Hz
            root_path: "/sys/class/hwmon".to_string(),
        }
    }
}

pub struct Metrics {
    pub power: TypedMetricId<u64>,
    pub energy: TypedMetricId<f64>,
}

impl Metrics {
    fn new(alumet: &mut AlumetPluginStart) -> anyhow::Result<Self> {
        let power = alumet.create_metric::<u64>(
            "grace_instant_power",
            PrefixedUnit::micro(Unit::Watt),
            "power consumption",
        )?;
        let energy = alumet.create_metric::<f64>(
            "grace_energy_consumption",
            PrefixedUnit::milli(Unit::Joule),
            "energy consumption (computed from the power)",
        )?;
        Ok(Self { power, energy })
    }
}
