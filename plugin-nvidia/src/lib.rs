use std::time::Duration;

use alumet::plugin::{rust::AlumetPlugin, ConfigTable};
use anyhow::anyhow;

#[cfg(feature = "jetson")]
mod jetson;
#[cfg(feature = "nvml")]
mod nvml;

struct NvidiaPlugin {
    poll_interval: Duration,
}

impl AlumetPlugin for NvidiaPlugin {
    fn name() -> &'static str {
        "nvidia"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO read from config
        let poll_interval = Duration::from_secs(1);
        Ok(Box::new(NvidiaPlugin { poll_interval }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        #[cfg(feature = "nvml")]
        self.start_nvml(alumet)?;

        #[cfg(feature = "jetson")]
        self.start_jetson(alumet)?;

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
        use alumet::pipeline::trigger::TriggerSpec;

        let nvml = nvml::NvmlDevices::detect(true)?;
        let stats = nvml.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!("No NVML-compatible GPU found."));
        }
        if stats.working_devices == 0 {
            return Err(anyhow!(
                "{} NVML-compatible devices found but none of them is working (see previous warnings).",
                stats.found_devices
            ));
        }

        for device in &nvml.devices {
            if let Some(device) = device {
                log::debug!(
                    "Found NVML device {} with features {:?}",
                    device.bus_id,
                    device.features
                );
            }
        }

        let metrics = nvml::Metrics::new(alumet)?;

        for maybe_device in nvml.devices {
            if let Some(device) = maybe_device {
                let source = nvml::NvmlSource::new(device, metrics.clone())?;
                alumet.add_source(Box::new(source), TriggerSpec::at_interval(self.poll_interval));
            }
        }
        Ok(())
    }

    /// Set up the collection of measurements on a Jetson edge device.
    ///
    /// This works by querying the embedded INA sensor(s).
    #[cfg(feature = "jetson")]
    fn start_jetson(&self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        use alumet::pipeline::trigger::TriggerSpec;

        let sensors = jetson::detect_ina_sensors()?;
        for sensor in &sensors {
            log::debug!("INA sensor found: {} at {}", sensor.i2c_id, sensor.path.display());
            for chan in &sensor.channels {
                let description = chan.description.as_deref().unwrap_or("?");
                log::debug!("\t- channel {} \"{}\": {}", chan.id, chan.label, description);
            }
        }
        let source = jetson::JetsonInaSource::open_sensors(sensors, alumet)?;
        alumet.add_source(Box::new(source), TriggerSpec::at_interval(self.poll_interval));
        Ok(())
    }
}
