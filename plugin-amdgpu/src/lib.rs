mod amd;
use amd::{probe::AmdGpuProbe, utils::*};
use amdsmi::{amdsmi_init, amdsmi_shut_down, AmdsmiInitFlagsT};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable,
    },
    units::{PrefixedUnit, Unit},
};

#[derive(Serialize, Deserialize)]
pub struct Config {
    /// Time between each activation of the counter source.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
}

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

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // Create the source
        let source = AmdGpuProbe::new(
            alumet.create_metric::<u64>(
                "amd_gpu_clock_frequency",
                PrefixedUnit::mega(Unit::Hertz),
                "Get GPU clock frequency in Mhz",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_energy_consumption",
                PrefixedUnit::milli(Unit::Joule),
                "Get GPU energy consumption in milliJoule",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_engine_usage",
                Unit::Unity,
                "Get GPU engine consumption in percentage",
            )?,
            alumet.create_metric::<u64>("amd_gpu_fan_speed", Unit::Unity, "Get GPU fans speed in percentage")?,
            alumet.create_metric::<u64>(
                "amd_gpu_memory_usage",
                PrefixedUnit::mega(Unit::Byte),
                "Get GPU used memory in MB",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_pci_data_sent",
                PrefixedUnit::kilo(Unit::Byte),
                "Get GPU PCI bus sent data in KB/s",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_pci_data_received",
                PrefixedUnit::kilo(Unit::Byte),
                "Get GPU PCI bus received data in KB/s",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_power_consumption",
                Unit::Watt,
                "Get GPU electric average power consumption in Watts",
            )?,
            alumet.create_metric::<u64>("amd_gpu_temperature", Unit::DegreeCelsius, "Get GPU temperature in °C")?,
            alumet.create_metric::<u64>(
                "amd_gpu_process_compute_counter",
                Unit::Unity,
                "Get GPU compute processes number",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_process_compute_unit_usage",
                Unit::Unity,
                "Get compute unit usage by process in percentage",
            )?,
            alumet.create_metric::<u64>(
                "amd_gpu_process_vram_usage",
                PrefixedUnit::mega(Unit::Byte),
                "Get VRAM memory usage by process in percentage",
            )?,
        );

        // Configure how the source is triggered: Alumet will call the source every 1s
        let trigger = trigger::builder::time_interval(self.config.poll_interval).build()?;

        // Add the source to the measurement pipeline
        let _ = alumet.add_source("amdgpu", Box::new(source), trigger);

        Ok(())
    }

    // Stop AMD GPU plugin and shut down the AMD SMI library.
    fn stop(&mut self) -> anyhow::Result<()> {
        amdsmi_shut_down()
            .map_err(AmdError)
            .context("Failed to shut down AMD SMI")
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    // Create a fake plugin structure for amdgpu plugin
    fn fake_config() -> AmdGpuPlugin {
        AmdGpuPlugin {
            config: Config {
                poll_interval: Duration::from_secs(1),
            },
        }
    }

    // Test `default_config` function of amdgpu plugin
    #[test]
    fn test_default_config() -> anyhow::Result<()> {
        let result = AmdGpuPlugin::default_config()?;
        let config_table = result.expect("default_config should return Some");
        let config: Config = deserialize_config(config_table).expect("Failed to deserialize config");
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        Ok(())
    }

    // Test `init` function to initialize amdgpu plugin configuration
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

    // Test `stop` function to stop amdgpu plugin
    #[test]
    fn test_stop() -> Result<()> {
        let mut plugin = fake_config();
        let result = plugin.stop();
        assert!(result.is_ok());
        Ok(())
    }
}
