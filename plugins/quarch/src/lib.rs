// This file contains the main implementation of the Quarch plugin for Alumet.
use alumet::{
    pipeline::elements::source::trigger,
    plugin::{
        AlumetPostStart, ConfigTable,
        event::{self},
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    process::Child,
    sync::{Arc, Mutex},
    time::Duration,
};

mod source;
use crate::source::QuarchSource;
use crate::source::SourceWrapper;

/// Structure for Quarch implementation
pub struct QuarchPlugin {
    config: Config,
    source: Option<Arc<Mutex<QuarchSource>>>,
    qis_process: Option<Child>,
}

/// Implementation of Quarch Plugin as an Alumet Plugin
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
        Ok(Box::new(QuarchPlugin {
            config,
            source: None,
            qis_process: None,
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        log::debug!("Starting Quarch plugin");

        self.qis_process = Some(QuarchSource::ensure_qis_running()?);

        let metric_id = alumet.create_metric::<f64>("disk_power", Unit::Watt, "Disk power consumption in Watts")?;

        let source = QuarchSource::new(
            self.config.quarch_ip,
            self.config.quarch_port,
            self.config.sample,
            // self.config.channel,
            metric_id,
        );

        let source = Arc::new(Mutex::new(source));
        self.source = Some(source.clone());

        let trigger = trigger::builder::time_interval(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .update_interval(self.config.flush_interval)
            .build()
            .unwrap();

        alumet.add_source(
            "quarch_source",
            Box::new(SourceWrapper { inner: source.clone() }),
            trigger,
        )?;

        Ok(())
    }

    fn pre_pipeline_start(&mut self, _alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, _alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let source = self.source.clone();
        event::end_consumer_measurement().subscribe(move |_evt| {
            if let Some(source) = &source
                && let Ok(mut s) = source.lock()
                && let Err(e) = s.stop_measurement()
            {
                log::error!("Error stopping QuarchSource measurement: {}", e);
            }
            Ok(())
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Stopping Quarch plugin (final cleanup)...");
        if let Some(source) = &self.source
            && let Ok(mut s) = source.lock()
            && let Err(e) = s.stop_measurement()
        {
            log::error!("Error stopping Quarch measurement: {}", e);
        }
        if let Some(mut child) = self.qis_process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::process::Command::new("pkill")
            .arg("-9")
            .arg("-f")
            .arg("qis.jar")
            .status();

        Ok(())
    }
}

impl Drop for QuarchPlugin {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub quarch_ip: IpAddr,
    pub quarch_port: u16,
    pub sample: u32,
    // pub channel: string,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            quarch_ip: IpAddr::from([172, 17, 30, 102]),
            quarch_port: 9760,
            sample: 32,
            // channel: 12V // Can be +12V, +3.3V or +3.3VAUX --> if we want to obtein a power of 6, it is better to stay on 12V channel for the Quarch Module on G5K (fixture QTL2347)
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
        }
    }
}
