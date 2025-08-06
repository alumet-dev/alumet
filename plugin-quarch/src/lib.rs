use alumet::{
    metrics::TypedMetricId,
    pipeline::elements::source::trigger,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPostStart, ConfigTable,
    },
    units::Unit,
};
use anyhow;
use log;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod quarchpy;
use crate::quarchpy::QuarchSource;

/// Structure for Quarch implementation
pub struct QuarchPlugin {
    config: Arc<Mutex<ParsedConfig>>,
}

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
        let parsed_config = ParsedConfig {
            metrics: config.metrics,
            quarch_ip: config.quarch_ip,
            quarch_port: config.quarch_port,
            poll_interval: config.poll_interval,
            flush_interval: config.flush_interval,
            metric_ids: Vec::new(),
        };
        Ok(Box::new(QuarchPlugin {
            config: Arc::new(Mutex::new(parsed_config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Starting Quarch plugin");

        let mut config = self.config.lock().unwrap();
        // Create metrics
        let mut metric_ids = Vec::with_capacity(config.metrics.len());
        for metric_name in &config.metrics {
            let metric_id = alumet.create_metric::<f64>(
                metric_name,
                Unit::Watt,
                format!("Disk power consumption for {}", metric_name),
            )?;
            metric_ids.push(metric_id);
        }
        config.metric_ids = metric_ids;

        if let Err(e) = quarchpy::start_quarch_measurement(&config.quarch_ip, config.quarch_port) {
            log::error!("Failed to start Quarch measurement: {}", e);
            return Err(e);
        }

        let source = QuarchSource::new(config.quarch_ip, config.quarch_port, config.metric_ids.clone());

        let trigger = trigger::builder::time_interval(config.poll_interval)
            .flush_interval(config.flush_interval)
            .update_interval(config.flush_interval)
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

/// A structure that stocks the configuration parameters that are necessary to ...
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub quarch_ip: IpAddr,
    pub quarch_port: u16,
    pub metrics: Vec<String>,

    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

struct ParsedConfig {
    quarch_ip: IpAddr,
    quarch_port: u16,
    metrics: Vec<String>,
    poll_interval: Duration,
    flush_interval: Duration,
    metric_ids: Vec<TypedMetricId<f64>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            quarch_ip: IpAddr::from([172, 17, 30, 102]),
            quarch_port: 8080,
            metrics: vec!["disk_power".to_string()],
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
        }
    }
}
