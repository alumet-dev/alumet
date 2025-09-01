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

        self.qis_process = Some(QuarchSource::ensure_qis_running(
            self.config.qis_port,
            &self.config.java_bin,
            &self.config.qis_jar_path,
        )?);

        let metric_id = alumet.create_metric::<f64>("disk_power", Unit::Watt, "Disk power consumption in Watts")?;

        let (sample, _) = poll_to_sample(self.config.poll_interval);
        // Convert to Quarch command format
        if sample >= 1024 {
            format!("{}K", sample / 1024)
        } else {
            sample.to_string()
        };

        let source = QuarchSource::new(self.config.quarch_ip, self.config.quarch_port, sample, metric_id);

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
/// Averaging table mapping sample → hardware window based on quarch documentation
struct Averaging {
    sample: u32,
    window: Duration,
}

const AVERAGING_TABLE: [Averaging; 11] = [
    Averaging {
        sample: 32,
        window: Duration::from_micros(130),
    },
    Averaging {
        sample: 64,
        window: Duration::from_micros(250),
    },
    Averaging {
        sample: 128,
        window: Duration::from_micros(500),
    },
    Averaging {
        sample: 256,
        window: Duration::from_micros(1_000),
    },
    Averaging {
        sample: 512,
        window: Duration::from_micros(2_000),
    },
    Averaging {
        sample: 1024,
        window: Duration::from_micros(4_100),
    },
    Averaging {
        sample: 2048,
        window: Duration::from_micros(8_200),
    },
    Averaging {
        sample: 4096,
        window: Duration::from_micros(16_400),
    },
    Averaging {
        sample: 8192,
        window: Duration::from_micros(32_800),
    },
    Averaging {
        sample: 16384,
        window: Duration::from_micros(65_500),
    },
    Averaging {
        sample: 32768,
        window: Duration::from_micros(131_000),
    },
];

/// Choose the best sample for quarch : the better for the poll_interval
pub fn poll_to_sample(poll: Duration) -> (u32, Duration) {
    for avg in AVERAGING_TABLE {
        if avg.window >= poll {
            return (avg.sample, avg.window);
        }
    }
    let last = AVERAGING_TABLE.last().unwrap();
    (last.sample, last.window)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub quarch_ip: IpAddr,
    pub quarch_port: u16,
    pub qis_port: u16,
    pub java_bin: String,
    pub qis_jar_path: String,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // --- Quarch connection settings ---
            quarch_ip: IpAddr::from([172, 17, 30, 102]), // By default for Grenoble G5K
            quarch_port: 9760,      // By default is you didn't change it on the module
            qis_port: 9780,         // By default is you didn't change it on the module
            java_bin: "/root/venv-quarchpy/lib/python3.11/site-packages/quarchpy/connection_specific/jdk_jres/lin_amd64_jdk_jre/bin/java".to_string(),
            qis_jar_path: "/root/venv-quarchpy/lib/python3.11/site-packages/quarchpy/connection_specific/QPS/win-amd64/qis/qis.jar".to_string(),
            // --- Measurement settings ---
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
        }
    }
}
