use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::Timestamp,
    pipeline::naming::SourceName,
    plugin::PluginMetadata,
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};
use plugin_grace_hopper::{Config, GraceHopperPlugin};
use std::time::Duration;
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(5);
const SOURCE_NAME: &str = "hwmon";
const METRIC_POWER: &str = "grace_instant_power";
const METRIC_ENERGY: &str = "grace_energy_consumption";

#[test]
fn plugin_without_device() {
    let root = tempdir().unwrap();
    let root_path = root.path().to_str().unwrap().to_string();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new().expect_source("grace-hopper", SOURCE_NAME);

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .build_and_start();
    assert!(agent.is_err(), "the plugin should fail to start (no hwmon device)")
}

#[test]
fn full_plugin_with_multiple_hwmon_sensors() {
    let root = tempdir().unwrap();

    let root_path = root.path().to_str().unwrap().to_string();
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "Module Power Socket 0").unwrap();
    std::fs::write(file_path_average, "60000000").unwrap();
    std::fs::write(file_path_interval, "50").unwrap();

    let file_path_info = root.path().join("hwmon2/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon2/device/power1_average");
    let file_path_interval = root.path().join("hwmon2/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "Grace Power Socket 0").unwrap();
    std::fs::write(file_path_average, "62000000").unwrap();
    std::fs::write(file_path_interval, "50").unwrap();

    let file_path_info = root.path().join("hwmon3/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon3/device/power1_average");
    let file_path_interval = root.path().join("hwmon3/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "CPU Power Socket 2").unwrap();
    std::fs::write(file_path_average, "64000000").unwrap();
    std::fs::write(file_path_interval, "100").unwrap();

    let file_path_info = root.path().join("hwmon6/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon6/device/power1_average");
    let file_path_interval = root.path().join("hwmon6/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "SysIO Power Socket 2").unwrap();
    std::fs::write(file_path_average, "67000000").unwrap();
    std::fs::write(file_path_interval, "77").unwrap();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
        .expect_metric::<u64>(METRIC_POWER, PrefixedUnit::micro(Unit::Watt))
        .expect_metric::<f64>(METRIC_ENERGY, PrefixedUnit::milli(Unit::Joule))
        .expect_source("grace-hopper", SOURCE_NAME);

    let source = SourceName::from_str("grace-hopper", SOURCE_NAME);
    let runtime_expectation = RuntimeExpectations::new()
        // call the source once, so that a "previous measurement" exists next time
        .test_source(
            source.clone(),
            || {},
            |ctx| {
                let m = ctx.measurements();
                let power_metric = ctx.metrics().by_name(METRIC_POWER).unwrap().0;
                let energy_metric = ctx.metrics().by_name(METRIC_ENERGY).unwrap().0;

                // we should have no energy for the moment
                assert!(
                    m.iter().find(|p| p.metric == energy_metric).is_none(),
                    "no energy metric should be pushed on the first poll"
                );

                // check the power measurements
                let m: Vec<_> = m.into_iter().filter(|p| p.metric == power_metric).collect();
                let module = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "module")
                        .is_some()
                });
                let grace = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "grace")
                        .is_some()
                });
                let cpu = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "cpu")
                        .is_some()
                });
                let sysio = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "sysio")
                        .is_some()
                });
                // TODO make it nicer to get an attribute by key
                assert_eq!(module.unwrap().value.as_u64(), 60000000);
                assert_eq!(grace.unwrap().value.as_u64(), 62000000);
                assert_eq!(cpu.unwrap().value.as_u64(), 64000000);
                assert_eq!(sysio.unwrap().value.as_u64(), 67000000);
                println!("t: {:?}", Timestamp::now());

                // We cannot sleep in the make_input of the next test_source, because the timestamp would not be updated. So, we sleep here.
                std::thread::sleep(Duration::from_secs(1))
            },
        )
        // call the source a second time and check the measurements
        .test_source(
            source.clone(),
            || {},
            |ctx| {
                println!("t: {:?}", Timestamp::now());
                let m = ctx.measurements();
                // check the energy measurements
                let energy_metric = ctx.metrics().by_name(METRIC_ENERGY).unwrap().0;
                let m: Vec<_> = m.into_iter().filter(|p| p.metric == energy_metric).collect();
                let module = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "module")
                        .is_some()
                });
                let grace = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "grace")
                        .is_some()
                });
                let cpu = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "cpu")
                        .is_some()
                });
                let sysio = m.iter().find(|p| {
                    p.attributes()
                        .find(|(k, v)| *k == "sensor" && v.to_string() == "sysio")
                        .is_some()
                });
                // TODO make it nicer to get an attribute by key

                // std::thread::sleep is not exact, and it's not the only thing that delays the source.
                // Apply a tolerance when checking the values.
                const TOLERANCE: f64 = 250.0;
                let module = module.unwrap().value.as_f64();
                let grace = grace.unwrap().value.as_f64();
                let cpu = cpu.unwrap().value.as_f64();
                let sysio = sysio.unwrap().value.as_f64();
                assert!(approx_eq(module, 60000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(grace, 62000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(cpu, 64000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(sysio, 67000.0, TOLERANCE), "bad value {module}");
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

fn approx_eq(a: f64, b: f64, epsilon: f64) -> bool {
    (b - a).abs() < epsilon
}
