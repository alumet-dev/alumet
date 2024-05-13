use std::time::Duration;

use alumet::{
    pipeline::trigger::TriggerSpec,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

#[cfg(feature = "jetson")]
mod jetson;
#[cfg(feature = "nvml")]
mod nvml;

pub struct NvidiaPlugin {
    config: Config,
}

impl AlumetPlugin for NvidiaPlugin {
    fn name() -> &'static str {
        "nvidia"
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
        Ok(Box::new(NvidiaPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        #[cfg(feature = "nvml")]
        self.start_nvml(alumet)?;

        #[cfg(feature = "jetson")]
        self.start_jetson(alumet)?;

        #[cfg(all(not(feature = "nvml"), not(feature = "jetson")))]
        compile_error!("To use the NVML plugin, please enable at least one of the following features: nvml, jetson");

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl NvidiaPlugin {
    /// Set up the collection of measurements with NVML.
    ///
    /// This works on a device that has a desktop-class or server-class NVIDIA GPU.
    /// For Jetson edge devices, use [`start_jetson`] instead.
    #[cfg(feature = "nvml")]
    fn start_nvml(&self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let nvml = nvml::NvmlDevices::detect(true)?;
        let stats = nvml.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!("No NVML-compatible GPU found. If your device is a Jetson edge device, please disable the `nvml` feature of the plugin."));
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

        let metrics = nvml::Metrics::new(alumet)?;

        for maybe_device in nvml.devices {
            if let Some(device) = maybe_device {
                let source = nvml::NvmlSource::new(device, metrics.clone())?;
                let trigger = TriggerSpec::builder(self.config.poll_interval)
                    .flush_interval(self.config.flush_interval)
                    .build()?;
                alumet.add_source(Box::new(source), trigger);
            }
        }
        Ok(())
    }

    /// Set up the collection of measurements on a Jetson edge device.
    ///
    /// This works by querying the embedded INA sensor(s).
    #[cfg(feature = "jetson")]
    fn start_jetson(&self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let sensors = jetson::detect_ina_sensors()?;
        if sensors.is_empty() {
            return Err(anyhow!("No INA sensor found. If you are not running on a Jetson device, disable the `jetson` feature of the plugin."));
        }

        for sensor in &sensors {
            log::info!("Found INA sensor {} at {}", sensor.i2c_id, sensor.path.display());
            for chan in &sensor.channels {
                let description = chan.description.as_deref().unwrap_or("?");
                log::debug!("\t- channel {} \"{}\": {}", chan.id, chan.label, description);
            }
        }
        let source = jetson::JetsonInaSource::open_sensors(sensors, alumet)?;
        let trigger = TriggerSpec::builder(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .build()?;
        alumet.add_source(Box::new(source), trigger);
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
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
