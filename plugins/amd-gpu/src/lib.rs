use crate::amd::utils::PLUGIN_NAME;
#[cfg(test)]
use alumet::plugin::PluginMetadata;
use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};
use amd::{device::AmdGpuDevices, metrics::Metrics, probe::AmdGpuSource};
use amd_smi_wrapper::MockableAmdSmi;
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

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

pub struct AmdGpuPlugin {
    pub config: Config,
    pub amdsmi: MockableAmdSmi,
}

impl AmdGpuPlugin {
    fn new(config: Config, amdsmi: MockableAmdSmi) -> Self {
        Self { config, amdsmi }
    }

    fn init_amdsmi() -> anyhow::Result<MockableAmdSmi> {
        #[cfg(test)]
        {
            use amd_smi_wrapper::MockAmdSmi;

            Ok(MockAmdSmi::new())
        }

        #[cfg(not(test))]
        {
            use amd_smi_wrapper::AmdSmi;

            Ok(AmdSmi::init()?)
        }
    }

    #[cfg(test)]
    pub fn test_metadata(amdsmi: MockableAmdSmi) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            init: Box::new(move |config_table| {
                let config = deserialize_config(config_table)?;
                Ok(Box::new(AmdGpuPlugin::new(config, amdsmi)))
            }),
            default_config: Box::new(AmdGpuPlugin::default_config),
        }
    }
}

impl AlumetPlugin for AmdGpuPlugin {
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
        let amdsmi = Self::init_amdsmi().context("Failed to initialize AMD SMI")?;
        Ok(Box::new(Self::new(config, amdsmi)))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let amdsmi = AmdGpuDevices::detect(&self.amdsmi, self.config.skip_failed_devices)?;

        let stats = amdsmi.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!("No AMDSMI-compatible GPU found."));
        }

        if stats.working_devices == 0 {
            return Err(anyhow!(
                "{} AMSMI-compatible devices found but none of them is working (see previous warnings).",
                stats.found_devices
            ));
        }

        for device in amdsmi.devices.iter() {
            log::info!(
                "Found AMD GPU device {} with features: {}",
                device.bus_id,
                device.features
            );
        }

        let metrics = Metrics::new(alumet)?;
        let source = AmdGpuSource::new(amdsmi.devices, metrics);
        let trigger = TriggerSpec::builder(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .build()?;

        alumet.add_source("amd_gpu_devices", Box::new(source), trigger)?;

        Ok(())
    }

    // Stopped automatically by AMD-SMI
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alumet::plugin::rust::AlumetPlugin;
    use amd_smi_wrapper::MockAmdSmi;
    use std::time::Duration;

    const CONFIGURATION: &str = r#"
        poll_interval = "1s"
        flush_interval = "5s"
        skip_failed_devices = true
        "#;

    // Test `default_config` function
    #[test]
    fn test_default_config() {
        let table = AmdGpuPlugin::default_config().expect("default_config() should not fail");
        let config: Config = deserialize_config(table.expect("default_config() should return Some")).unwrap();

        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.flush_interval, Duration::from_secs(5));
        assert_eq!(config.skip_failed_devices, true);
    }

    // Test `init` function to initialize the plugin and the amd smi library
    #[test]
    fn test_init_success() {
        let config: Config = toml::from_str(CONFIGURATION).unwrap();
        let config_table = serialize_config(config).unwrap();
        let plugin = AmdGpuPlugin::init(config_table).unwrap();

        assert_eq!(plugin.config.poll_interval, Duration::from_secs(1));
        assert_eq!(plugin.config.flush_interval, Duration::from_secs(5));
        assert_eq!(plugin.config.skip_failed_devices, true);
    }

    // Test `stop` function
    #[test]
    fn test_stop_success() {
        let mut mock = MockAmdSmi::new();
        mock.expect_stop().returning(|| Ok(()));

        let mut plugin = AmdGpuPlugin {
            config: Config::default(),
            amdsmi: mock,
        };

        assert!(plugin.stop().is_ok());
    }
}
