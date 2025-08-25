use amdsmi::{AmdsmiInitFlagsT, amdsmi_init, amdsmi_shut_down};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger,
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

mod amd;
use amd::{device::AmdDevices, error::AmdError, metrics::Metrics, probe::AmdGpuSource};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

pub struct AmdGpuPlugin {
    config: Config,
}

impl AlumetPlugin for AmdGpuPlugin {
    // Name of plugin, in lowercase, without the "plugin-" prefix
    fn name() -> &'static str {
        "amdgpu"
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
        amdsmi_init(AmdsmiInitFlagsT::AmdsmiInitAmdGpus)
            .map_err(AmdError)
            .context("Failed to initialize AMD SMI")?;
        Ok(Box::new(AmdGpuPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let amdsmi = AmdDevices::detect(self.config.skip_failed_devices)?;
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

        let metrics = Metrics::new(alumet)?;
        for device in amdsmi.devices.into_iter().flatten() {
            let source_name = format!("device_{}", device.bus_id);
            let source = AmdGpuSource::new(device, metrics.clone()).map_err(AmdError)?;
            let trigger = trigger::builder::time_interval(self.config.poll_interval).build()?;
            alumet.add_source(&source_name, Box::new(source), trigger)?;
        }
        Ok(())
    }

    // Stop AMD GPU plugin and shut down the AMD SMI library.
    fn stop(&mut self) -> anyhow::Result<()> {
        amdsmi_shut_down()
            .map_err(AmdError)
            .context("Failed to shut down AMD SMI")
    }
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Initial interval between two Nvidia measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    /// On startup, the plugin inspects the GPU devices and detect their features.
    /// If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
    /// If `skip_failed_devices = false`, the first failure will make the plugin's startup fail.
    #[serde(default = "default_true")]
    skip_failed_devices: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            skip_failed_devices: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    // Create a mock plugin structure for amd-gpu plugin
    fn mock_config() -> AmdGpuPlugin {
        AmdGpuPlugin {
            config: Config {
                poll_interval: Duration::from_secs(1),
                skip_failed_devices: true,
            },
        }
    }

    // Test `init` function to initialize amd-gpu plugin configuration
    #[test]
    fn test_init() -> anyhow::Result<()> {
        let config_table = serialize_config(Config::default())?;
        let result = AmdGpuPlugin::init(config_table);

        if let Err(e) = result {
            assert!(format!("{e:#}").contains("Failed to initialize AMD SMI"));
        } else {
            assert!(result.is_ok());
        }

        Ok(())
    }

    // Test `stop` function to stop amd-gpu plugin
    #[test]
    fn test_stop() -> Result<()> {
        let mut plugin = mock_config();
        let result = plugin.stop();
        assert!(result.is_ok());
        Ok(())
    }
}
