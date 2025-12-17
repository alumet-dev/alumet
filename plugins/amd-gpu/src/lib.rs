#[cfg(not(test))]
use crate::interface::AmdSmiLib;
#[cfg(test)]
use crate::interface::MockAmdSmiLibProvider;

use crate::{
    amd::utils::PLUGIN_NAME,
    interface::{AmdError, AmdSmiLibProvider},
};
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
    pub amdsmi: Box<dyn AmdSmiLibProvider>,
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

        #[cfg(not(test))]
        let amdsmi: Box<dyn AmdSmiLibProvider> =
            <AmdSmiLib as AmdSmiLibProvider>::lib_init().context("Failed to initialize AMD SMI")?;
        #[cfg(test)]
        let amdsmi: Box<dyn AmdSmiLibProvider> = Box::new(MockAmdSmiLibProvider::new());

        Ok(Box::new(Self { config, amdsmi }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let amdsmi = AmdGpuDevices::detect(self.amdsmi.as_ref(), self.config.skip_failed_devices)?;

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

    fn stop(&mut self) -> anyhow::Result<()> {
        self.amdsmi.lib_stop().context("Failed to shut down AMD SMI")
    }
}

#[cfg(test)]
mod tests_lib {
    use super::*;
    use alumet::plugin::rust::AlumetPlugin;
    use amd::utils::ERROR;
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
        let _mock = AmdGpuPlugin::init(config_table).unwrap();
    }

    // Test `stop` function in success case
    #[test]
    fn test_stop_success() {
        let mut mock = MockAmdSmiLibProvider::new();
        mock.expect_lib_stop().returning(|| Ok(()));

        let mut plugin = AmdGpuPlugin {
            config: Config::default(),
            amdsmi: Box::new(mock),
        };

        assert!(plugin.stop().is_ok());
    }

    // Test `stop` function in error case
    #[test]
    fn test_stop_error() {
        let mut mock = MockAmdSmiLibProvider::new();
        mock.expect_lib_stop().returning(|| Err(AmdError(ERROR)));

        let mut plugin = AmdGpuPlugin {
            config: Config::default(),
            amdsmi: Box::new(mock),
        };

        assert!(plugin.stop().is_err());
    }
}
