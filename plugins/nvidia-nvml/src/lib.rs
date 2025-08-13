use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

mod nvml;

pub struct NvmlPlugin {
    config: Config,
}

impl AlumetPlugin for NvmlPlugin {
    fn name() -> &'static str {
        "nvml"
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
        Ok(Box::new(NvmlPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let nvml = nvml::device::NvmlDevices::detect(true)?;
        let stats = nvml.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!(
                "No NVML-compatible GPU found. If your device is a Jetson edge device, please disable the `nvml` feature of the plugin."
            ));
        }
        if stats.working_devices == 0 {
            return Err(anyhow!(
                "{} NVML-compatible devices found but none of them is working (see previous warnings).",
                stats.found_devices
            ));
        }

        for device in &nvml.devices {
            if let Some(device) = device {
                let device_name = device
                    .as_wrapper()
                    .name()
                    .with_context(|| format!("failed to get the name of NVML device {}", device.bus_id))?;
                log::info!(
                    "Found NVML device {} \"{}\" with features: {}",
                    device.bus_id,
                    device_name,
                    device.features
                );
            }
        }
        let metrics = nvml::metrics::Metrics::new(alumet)?;

        for maybe_device in nvml.devices {
            if let Some(device) = maybe_device {
                let source_name = format!("device_{}", device.bus_id);
                let source = nvml::probe::NvmlSource::new(device, metrics.clone())?;
                let trigger = TriggerSpec::builder(self.config.poll_interval)
                    .flush_interval(self.config.flush_interval)
                    .build()?;
                alumet.add_source(&source_name, Box::new(source), trigger)?;
            }
        }
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
