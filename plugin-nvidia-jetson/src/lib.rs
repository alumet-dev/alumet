mod ina;
mod source;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};

pub struct JetsonPlugin {
    config: Config,
}

impl AlumetPlugin for JetsonPlugin {
    fn name() -> &'static str {
        "jetson"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(JetsonPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let (mut sensors, errs) =
            ina::detect_ina_sensors().context("no INA-3221 sensor found, are you running on a Jetson device?")?;
        ina::sort_sensors_recursively(&mut sensors);

        // print errors to help the admin
        log::warn!(
            "Some errors happened during the detection of INA-3221 sensors. Nevertheless, the plugin will continue."
        );
        for err in errs {
            log::warn!("  - {err}");
        }

        // print valid sensors
        for sensor in &sensors {
            log::info!("Found INA-3221 sensor {}", sensor.metadata);
            for chan in &sensor.channels {
                let description = chan.label.as_deref().unwrap_or("?");
                log::debug!(
                    "  - channel {} \"{}\": {}",
                    chan.id,
                    chan.label.as_deref().unwrap_or("?"),
                    description
                );
            }
        }

        // prepare the measurement source
        let source = source::JetsonInaSource::open_sensors(sensors, alumet)?;
        let trigger = TriggerSpec::builder(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .build()?;
        alumet.add_source("builtin_ina", Box::new(source), trigger)?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Initial interval between two Nvidia measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// Initial interval between two flushing of Nvidia measurements.
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
        }
    }
}
