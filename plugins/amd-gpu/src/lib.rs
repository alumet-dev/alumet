use crate::amd::utils::PLUGIN_NAME;
use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};
use amd::{device::AmdGpuDevices, metrics::Metrics, probe::AmdGpuSource};
use amd_smi_wrapper::{AmdInterface, AmdSmi};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};

#[cfg(test)]
use alumet::plugin::PluginMetadata;
#[cfg(test)]
use amd_smi_wrapper::MockAmdInterface;

mod amd;
mod tests;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Initial interval between two AMD GPU measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    /// Initial interval between two flushing of AMD GPU measurements.
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,
    /// On startup, the plugin inspects the GPU devices and detect their features.
    /// If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
    /// If `skip_failed_devices = false`, the first failure will make the plugin's startup fail.
    pub skip_failed_devices: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
            skip_failed_devices: true,
        }
    }
}

pub struct AmdGpuPlugin<P: AmdInterfaceProvider> {
    pub config: Config,
    pub amdsmi: P::A,
}

pub trait AmdInterfaceProvider {
    type A: AmdInterface;
    fn get() -> anyhow::Result<Self::A>;
}

pub struct AmdSmiProvider;

impl AmdInterfaceProvider for AmdSmiProvider {
    type A = Arc<AmdSmi>;

    fn get() -> anyhow::Result<Arc<AmdSmi>> {
        Ok(AmdSmi::init()?)
    }
}

impl<P: AmdInterfaceProvider + 'static> AlumetPlugin for AmdGpuPlugin<P> {
    fn name() -> &'static str {
        PLUGIN_NAME
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
        let amdsmi = P::get().context("Failed to initialize AMD SMI")?;
        Ok(Box::new(Self { config, amdsmi }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let amdsmi = AmdGpuDevices::detect(&self.amdsmi, self.config.skip_failed_devices)?;

        let stats = amdsmi.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!("No AMDSMI-compatible GPU found."));
        }

        if stats.working_devices == 0 {
            let found = stats.found_devices;
            let msg = format!(
                "{} AMSMI-compatible devices found but none of them is working (see previous warnings).",
                &found
            );
            return Err(anyhow!(msg));
        }

        for device in amdsmi.devices.iter() {
            let bus_id = &device.bus_id;
            let features = &device.features;
            log::info!("Found AMD GPU device {bus_id} with features: {features}");
        }

        let metrics = Metrics::new(alumet)?;
        let source = AmdGpuSource::new(amdsmi.devices, metrics);
        let trigger = TriggerSpec::builder(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .build()?;

        alumet.add_source("amd_gpu_devices", Box::new(source), trigger)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // The AMD-SMI library is automatically dropped when the source is dropped
        Ok(())
    }
}

#[cfg(test)]
struct MockProvider;

#[cfg(test)]
impl AmdInterfaceProvider for MockProvider {
    type A = MockAmdInterface;

    fn get() -> anyhow::Result<Self::A> {
        Ok(MockAmdInterface::new())
    }
}

#[cfg(test)]
impl AmdGpuPlugin<MockProvider> {
    pub fn test_metadata(amdsmi: MockAmdInterface) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            init: Box::new(move |config_table| {
                let config = deserialize_config(config_table)?;
                Ok(Box::new(Self { config, amdsmi }))
            }),
            default_config: Box::new(Self::default_config),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // Test `default_config` function
    #[test]
    fn test_default_config() {
        let table = AmdGpuPlugin::<MockProvider>::default_config().expect("default_config() should not fail");
        let config: Config = deserialize_config(table.expect("default_config() should return Some")).unwrap();

        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.flush_interval, Duration::from_secs(5));
    }

    // Test `init` function with amd-smi init call
    #[test]
    fn test_init_executes_amdsmi_get() -> Result<(), Box<dyn std::error::Error>> {
        let config_table: ConfigTable = serialize_config(Config::default())?;
        let plugin = AmdGpuPlugin::<MockProvider>::init(config_table)?;

        assert_eq!(plugin.config.poll_interval, Duration::from_secs(1));
        assert_eq!(plugin.config.flush_interval, Duration::from_secs(5));
        Ok(())
    }
}
