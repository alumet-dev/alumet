use alumet::plugin::rust::AlumetPlugin;
use anyhow::anyhow;

#[cfg(feature = "jetson")]
mod jetson;
#[cfg(feature = "nvml")]
mod nvml;

struct NvidiaPlugin;

impl AlumetPlugin for NvidiaPlugin {
    fn name() -> &'static str {
        "nvidia"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn init(_config: &mut alumet::config::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(NvidiaPlugin))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        #[cfg(feature = "nvml")]
        start_nvml(alumet)?;

        #[cfg(feature = "jetson")]
        start_jetson(alumet)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Set up the collection of measurements with NVML.
///
/// This works on a device that has a desktop-class or server-class NVIDIA GPU.
/// For Jetson edge devices, use [`start_jetson`] instead.
#[cfg(feature = "nvml")]
fn start_nvml(alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
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
            alumet.add_source(Box::new(source));
        }
    }
    Ok(())
}

/// Set up the collection of measurements on a Jetson edge device.
///
/// This works by querying the embedded INA sensor(s).
#[cfg(feature = "jetson")]
fn start_jetson(alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
    let sensors = jetson::detect_ina_sensors()?;
    for sensor in &sensors {
        log::debug!("INA sensor found: {} at {}", sensor.i2c_id, sensor.path.display());
        for chan in &sensor.channels {
            let description = chan.description.as_deref().unwrap_or("?");
            log::debug!("\t- channel {} \"{}\": {}", chan.id, chan.label, description);
        }
    }
    let source = jetson::JetsonInaSource::open_sensors(sensors, alumet)?;
    alumet.add_source(Box::new(source));
    Ok(())
}
