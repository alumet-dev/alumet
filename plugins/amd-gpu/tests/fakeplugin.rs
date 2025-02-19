use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    pipeline::naming::SourceName,
    plugin::{
        PluginMetadata,
        rust::{AlumetPlugin, serialize_config},
    },
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};
use std::{panic, time::Duration};
use anyhow::Result;

use plugin_amd_gpu::{AmdGpuPlugin, Config};

// Create a mock plugin structure for amd-gpu plugin
fn mock_plugin() -> AmdGpuPlugin {
    AmdGpuPlugin {
        config: Config {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(0),
            skip_failed_devices: true,
        },
    }
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

// Test `init` function to initialize amd-gpu plugin configuration
#[test]
fn test_init() -> anyhow::Result<()> {
    let config_table = serialize_config(Config::default())?;
    let res = AmdGpuPlugin::init(config_table);

    if let Err(e) = res {
        assert!(format!("{e:#}").contains("Failed to initialize AMD SMI"));
    } else {
        assert!(res.is_ok());
    }

    Ok(())
}

// Test `stop` function to stop amd-gpu plugin
#[test]
fn test_stop() -> Result<()> {
    let mut plugin = mock_plugin();
    let res = plugin.stop();

    if let Err(e) = res {
        assert!(format!("{e:#}").contains("Failed to shut down AMD SMI"));
    } else {
        assert!(res.is_ok());
    }

    Ok(())
}

// Test `start` function for amd-gpu plugin metric collect with correct values
#[test]
fn test_start_success() {
    let mut plugins = PluginSet::new();
    let config = Config {
        ..Default::default()
    };

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectations = StartupExpectations::new()
        .expect_metric::<f64>("amd_gpu_energy_consumption", PrefixedUnit::milli(Unit::Joule))
        .expect_metric::<f64>("amd_gpu_engine_usage", Unit::Percent.clone())
        .expect_metric::<u64>("amd_gpu_memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("amd_gpu_power_consumption", Unit::Watt.clone())
        .expect_metric::<u64>("amd_gpu_temperature", Unit::DegreeCelsius.clone())
        .expect_metric::<u64>("amd_gpu_process_memory_usage", Unit::Byte.clone())
        .expect_metric::<u64>("amd_gpu_process_engine_usage_encode", PrefixedUnit::nano(Unit::Second))
        .expect_metric::<u64>("amd_gpu_process_engine_gfx", PrefixedUnit::nano(Unit::Second))
        .expect_metric::<u64>("amd_gpu_process_memory_usage_gtt", Unit::Byte.clone())
        .expect_metric::<u64>("amd_gpu_process_memory_usage_cpu", Unit::Byte.clone())
        .expect_metric::<u64>("amd_gpu_process_memory_usage_vram", Unit::Byte.clone());

    // TODO : Check that sources are correct
    let run_expect = RuntimeExpectations::new()
        .test_source(
            SourceName::from_str("amd-gpu", "amd_gpu_energy_consumption"),
            move || {},
            |_output| {},
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations)
        .with_expectations(run_expect)
        .build_and_start()
        .unwrap();

    // Send shutdown message
    agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
}

// Test `start` function with no AMD GPUs detected
#[test]
fn test_start_errors() {
    let res = panic::catch_unwind(|| {
        let mut plugins = PluginSet::new();
        let config = Config {
            ..Default::default()
        };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        // Send nothing
        let startup_expectations = StartupExpectations::new();

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
    });

    assert!(res.is_err());
}
