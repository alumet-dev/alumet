use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::{Source, elements::source::trigger::TriggerSpec},
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

use crate::{
    metrics::{FullMetrics, MinimalMetrics},
    probe::SourceProvider,
};

mod device;
mod features;
mod metrics;
mod nvml_ext;
mod probe;

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
        let nvml = device::NvmlDevices::detect(self.config.skip_failed_devices)?;
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
        let source_provider = match self.config.mode {
            Mode::Full => SourceProvider::Full(FullMetrics::new(alumet)?),
            Mode::Minimal => SourceProvider::Minimal(MinimalMetrics::new(alumet)?),
        };

        for maybe_device in nvml.devices {
            if let Some(device) = maybe_device {
                let source_name = format!("device_{}", device.bus_id);
                let trigger = TriggerSpec::builder(self.config.poll_interval)
                    .flush_interval(self.config.flush_interval)
                    .build()?;

                let source: Box<dyn Source> = match &source_provider {
                    SourceProvider::Full(metrics) => Box::new(probe::FullSource::new(device, metrics.clone())?),
                    SourceProvider::Minimal(metrics) => Box::new(probe::MinimalSource::new(device, metrics.clone())?),
                };
                alumet.add_source(&source_name, source, trigger)?;
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

    /// On startup, the plugin inspects the GPU devices and detect their features.
    /// If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
    /// If `skip_failed_devices = true`, the first failure will make the plugin's startup fail.
    #[serde(default = "default_true")]
    skip_failed_devices: bool,

    /// In "full" mode, get many measurements from the GPU on each poll.
    /// In "minimal" mode, only measure the power consumption (it must be supported by the GPU).
    ///
    /// On some GPUs, the "full" mode is too slow for high frequencies (100 Hz can be hard to reach in full mode).
    mode: Mode,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    /// Gathers many NVML metrics.
    Full,
    /// Only measure the power consumption, and estimate the energy from the power.
    Minimal,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            skip_failed_devices: true,
            mode: Mode::Full,
        }
    }
}
