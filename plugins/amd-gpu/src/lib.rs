#[cfg(not(test))]
use crate::interface::AmdSmiLib;
#[cfg(test)]
use crate::interface::MockAmdSmiLib;
use crate::{amd::utils::PLUGIN_NAME, interface::AmdError};
use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

mod amd;
pub mod bindings;
mod interface;
pub mod tests;

use amd::{device::AmdGpuDevices, metrics::Metrics, probe::AmdGpuSource};

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
    #[cfg(not(test))]
    pub amdsmi: interface::AmdSmiLib,
    #[cfg(test)]
    pub amdsmi: interface::MockAmdSmiLib,
}

impl AlumetPlugin for AmdGpuPlugin {
    // Name of plugin, in lowercase, without the "plugin-" prefix
    fn name() -> &'static str {
        PLUGIN_NAME
    }

    // Gets the version from the Cargo.toml of the plugin crate
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    // Initialization of AMD GPU and AMD SMI library.
    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        #[cfg(not(test))]
        let amdsmi = AmdSmiLib::init().context("Failed to initialize AMD SMI")?;
        #[cfg(test)]
        let amdsmi = MockAmdSmiLib::new();
        Ok(Box::new(AmdGpuPlugin { config, amdsmi }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        #[cfg(test)]
        let amdsmi = AmdGpuDevices::detect(&self.amdsmi, self.config.skip_failed_devices)?;
        #[cfg(not(test))]
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
        for device in amdsmi.devices.into_iter() {
            let source_name = format!("device_{}", device.bus_id);
            let source = AmdGpuSource::new(device, metrics.clone()).map_err(AmdError)?;
            let trigger = TriggerSpec::builder(self.config.poll_interval)
                .flush_interval(self.config.flush_interval)
                .build()?;
            alumet.add_source(&source_name, Box::new(source), trigger)?;
        }
        Ok(())
    }

    // Stop AMD GPU plugin and shut down the AMD SMI library.
    fn stop(&mut self) -> anyhow::Result<()> {
        self.amdsmi.stop().context("Failed to shut down AMD SMI")
    }
}

#[cfg(test)]
mod tests_lib {
    use super::*;

    #[cfg(test)]
    use crate::bindings::{amdsmi_status_t, amdsmi_status_t_AMDSMI_STATUS_INVAL};
    use alumet::plugin::rust::AlumetPlugin;
    use std::time::Duration;

    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

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

    #[test]
    fn test_init() {
        let config: Config = toml::from_str(CONFIGURATION).unwrap();
        let config_table = serialize_config(config).unwrap();

        // Test `init` function to initialize the plugin and the amd smi library
        let mut mock = AmdGpuPlugin::init(config_table).unwrap();
        mock.amdsmi.checkpoint();

        // Test `stop` function in success case
        mock.amdsmi.expect_stop().returning(|| Ok(()));
        assert!(mock.stop().is_ok());
        mock.amdsmi.checkpoint();

        // Test `stop` function in error case
        mock.amdsmi.expect_stop().returning(|| Err(AmdError(ERROR)));
        assert!(mock.stop().is_err());
        mock.amdsmi.checkpoint();
    }
}
