// This file contains the main implementation of the Quarch plugin for Alumet.

use alumet::{
    pipeline::elements::source::trigger,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPostStart, ConfigTable,
    },
    units::Unit,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::time::Duration;

mod quarchpy;
use crate::quarchpy::QuarchSource;

/// Structure for Quarch implementation
pub struct QuarchPlugin {
    config: Config,
}

/// Implementation of Quarch plugin as an Alumet plugin
impl AlumetPlugin for QuarchPlugin {
    fn name() -> &'static str {
        "quarch"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        pyo3::prepare_freethreaded_python(); // to force init of python interpretor
        Ok(Box::new(QuarchPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Starting Quarch plugin");

        let metric_id = alumet.create_metric::<f64>("disk_power", Unit::Watt, "Disk power consumption in Watts")?;

        if let Err(e) = quarchpy::start_quarch_measurement(&self.config.quarch_ip, self.config.quarch_port) {
            log::error!("Failed to start Quarch measurement: {}", e);
            return Err(e);
        }

        // We create a Source for Quarch
        let source = QuarchSource::new(self.config.quarch_ip, self.config.quarch_port, metric_id);

        let trigger = trigger::builder::time_interval(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .update_interval(self.config.flush_interval)
            .build()
            .unwrap();

        alumet.add_source("quarch_source", Box::new(source), trigger)?;
        Ok(())
    }

    fn pre_pipeline_start(&mut self, _alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, _alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// A structure that stocks the configuration parameters
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub quarch_ip: IpAddr,
    pub quarch_port: u16,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            quarch_ip: IpAddr::from([172, 17, 30, 102]),
            quarch_port: 8080,
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
        }
    }
}
