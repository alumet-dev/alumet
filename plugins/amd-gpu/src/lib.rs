use crate::bindings::{amdsmi_init_flags_t, amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS, libamd_smi};
use anyhow::{Context, Error, anyhow};
use serde::{Deserialize, Serialize};
use std::{sync::OnceLock, time::Duration};

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

mod amd;
use amd::{device::AmdGpuDevices, error::AmdError, metrics::Metrics, probe::AmdGpuSource, utils::*};
pub mod bindings;
pub mod tests;

const LIB_PATH: &str = "libamd_smi.so";
static AMDSMI_INSTANCE: OnceLock<libamd_smi> = OnceLock::new();

/// Use a [`OnceLock`] instance of the AMD SMI library deployment,
/// if it is not already initialised by a thread.
///
/// # Return
///
/// - Ok if already initialised by an other thread
pub fn load_amdsmi() -> Result<&'static libamd_smi, Error> {
    if let Some(instance) = AMDSMI_INSTANCE.get() {
        return Ok(instance);
    }

    let amdsmi = unsafe { libamd_smi::new(LIB_PATH) }.map_err(|e| anyhow!("Failed to load AMD SMI library: {e}"))?;
    let _ = AMDSMI_INSTANCE.set(amdsmi);
    Ok(AMDSMI_INSTANCE
        .get()
        .expect("AMD SMI library loading initialized but missing value"))
}

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
        const INIT_FLAG: amdsmi_init_flags_t = amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS;

        amd_sys_init(load_amdsmi()?, INIT_FLAG)
            .map_err(AmdError)
            .context("Failed to initialize AMD SMI")?;

        Ok(Box::new(AmdGpuPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let amdsmi = AmdGpuDevices::detect(self.config.skip_failed_devices)?;
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
        amd_sys_shutdown(load_amdsmi()?)
            .map_err(AmdError)
            .context("Failed to shut down AMD SMI")
    }
}

#[cfg(test)]
mod tests_lib {
    use super::*;
    use crate::bindings::{
        amdsmi_status_t, amdsmi_status_t_AMDSMI_STATUS_INVAL, amdsmi_status_t_AMDSMI_STATUS_SUCCESS,
    };
    use alumet::plugin::rust::{AlumetPlugin, serialize_config};
    use std::time::Duration;

    use crate::tests::common::ffi_mock::ffi_mocks_init::set_mock_init;

    const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
    const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;

    // Test `default_config` function
    #[test]
    fn test_default_config() {
        let table = AmdGpuPlugin::default_config()
            .expect("default_config() should not fail")
            .expect("default_config() should return Some");

        let config: Config = deserialize_config(table).unwrap();
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.flush_interval, Duration::from_secs(5));
        assert_eq!(config.skip_failed_devices, true);
    }

    // Test `init` function in success case
    #[test]
    fn test_init_success() {
        set_mock_init(SUCCESS);

        let table = serialize_config(Config::default()).unwrap();
        let plugin = AmdGpuPlugin::init(table);
        assert!(plugin.is_ok());
    }

    // Test `init` function in error case
    #[test]
    fn test_init_failure() {
        set_mock_init(ERROR);

        let table = serialize_config(Config::default()).unwrap();
        let plugin = AmdGpuPlugin::init(table);
        assert!(plugin.is_err());
    }

    // Test `stop` function in success case
    #[test]
    fn test_stop_success() {
        set_mock_init(SUCCESS);
        let mut plugin = AmdGpuPlugin {
            config: Config::default(),
        };
        assert!(plugin.stop().is_ok());
    }

    // Test `stop` function in error case
    #[test]
    fn test_stop_failure() {
        set_mock_init(ERROR);
        let mut plugin = AmdGpuPlugin {
            config: Config::default(),
        };
        assert!(plugin.stop().is_err());
    }
}
