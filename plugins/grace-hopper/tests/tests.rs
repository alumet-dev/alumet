use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::{MeasurementPoint, Timestamp},
    pipeline::naming::SourceName,
    plugin::PluginMetadata,
    resources::Resource,
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
    // Socket 0
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "Module Power Socket 0").unwrap();
    std::fs::write(file_path_average, "80000000").unwrap();
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
    std::fs::write(file_path_info, "CPU Power Socket 0").unwrap();
    std::fs::write(file_path_average, "42000000").unwrap();
    std::fs::write(file_path_interval, "100").unwrap();

    let file_path_info = root.path().join("hwmon6/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon6/device/power1_average");
    let file_path_interval = root.path().join("hwmon6/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "SysIO Power Socket 0").unwrap();
    std::fs::write(file_path_average, "18000000").unwrap();
    std::fs::write(file_path_interval, "77").unwrap();

    // Socket 1
    let file_path_info = root.path().join("hwmon4/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon4/device/power1_average");
    let file_path_interval = root.path().join("hwmon4/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "Grace Power Socket 1").unwrap();
    std::fs::write(file_path_average, "40000000").unwrap();
    std::fs::write(file_path_interval, "50").unwrap();

    let file_path_info = root.path().join("hwmon5/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon5/device/power1_average");
    let file_path_interval = root.path().join("hwmon5/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "CPU Power Socket 1").unwrap();
    std::fs::write(file_path_average, "35000000").unwrap();
    std::fs::write(file_path_interval, "100").unwrap();

    let file_path_info = root.path().join("hwmon7/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon7/device/power1_average");
    let file_path_interval = root.path().join("hwmon7/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    std::fs::write(file_path_info, "Module Power Socket 1").unwrap();
    std::fs::write(file_path_average, "5000000").unwrap();
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
                println!("t: {:?}", Timestamp::now());

                // we should have no energy for the moment
                assert!(
                    m.iter().find(|p| p.metric == energy_metric).is_none(),
                    "no energy metric should be pushed on the first poll"
                );

                // check the power measurements
                let m: Vec<_> = m.into_iter().filter(|p| p.metric == power_metric).collect();

                let module_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "module").unwrap();
                let module_1 = get_point(&m, Resource::CpuPackage { id: 1 }, "module").unwrap();
                let module_total = get_point(&m, Resource::LocalMachine, "module_total").unwrap();
                assert_eq!(module_0.value.as_u64(), 80000000);
                assert_eq!(module_1.value.as_u64(), 5000000);
                assert_eq!(
                    module_total.value.as_u64(),
                    module_0.value.as_u64() + module_1.value.as_u64()
                );

                let grace_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "grace").unwrap();
                let grace_1 = get_point(&m, Resource::CpuPackage { id: 1 }, "grace").unwrap();
                let grace_total = get_point(&m, Resource::LocalMachine, "grace_total").unwrap();
                assert_eq!(grace_0.value.as_u64(), 62000000);
                assert_eq!(grace_1.value.as_u64(), 40000000);
                assert_eq!(
                    grace_total.value.as_u64(),
                    grace_0.value.as_u64() + grace_1.value.as_u64()
                );

                let cpu_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "cpu").unwrap();
                let cpu_1 = get_point(&m, Resource::CpuPackage { id: 1 }, "cpu").unwrap();
                let cpu_total = get_point(&m, Resource::LocalMachine, "cpu_total").unwrap();
                assert_eq!(cpu_0.value.as_u64(), 42000000);
                assert_eq!(cpu_1.value.as_u64(), 35000000);
                assert_eq!(cpu_total.value.as_u64(), cpu_0.value.as_u64() + cpu_1.value.as_u64());

                let sysio_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "sysio").unwrap();
                assert!(
                    get_point(&m, Resource::CpuPackage { id: 1 }, "sysio").is_none(),
                    "there should be no sysio_1"
                );
                let sysio_total = get_point(&m, Resource::LocalMachine, "sysio_total").unwrap();
                assert_eq!(sysio_0.value.as_u64(), 18000000);
                assert_eq!(sysio_total.value.as_u64(), sysio_0.value.as_u64());

                let dram_0 = get_point(&m, Resource::Dram { pkg_id: 0 }, "dram").unwrap();
                assert!(
                    get_point(&m, Resource::Dram { pkg_id: 1 }, "dram").is_none(),
                    "there should be no dram_1 because there is no sysio_1"
                );
                let dram_total = get_point(&m, Resource::LocalMachine, "dram_total").unwrap();
                assert_eq!(dram_total.value.as_u64(), dram_0.value.as_u64());

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

                let energy_module_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "module").unwrap();
                let energy_grace_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "grace").unwrap();
                let energy_cpu_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "cpu").unwrap();
                let energy_sysio_0 = get_point(&m, Resource::CpuPackage { id: 0 }, "sysio").unwrap();
                let energy_dram_0 = get_point(&m, Resource::Dram { pkg_id: 0 }, "dram").unwrap();

                // std::thread::sleep is not exact, and it's not the only thing that delays the source.
                // Apply a tolerance when checking the values.
                const TOLERANCE: f64 = 250.0;
                let module = energy_module_0.value.as_f64();
                let grace = energy_grace_0.value.as_f64();
                let cpu = energy_cpu_0.value.as_f64();
                let sysio = energy_sysio_0.value.as_f64();
                let dram = energy_dram_0.value.as_f64();
                assert!(approx_eq(module, 80000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(grace, 62000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(cpu, 42000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(sysio, 18000.0, TOLERANCE), "bad value {module}");
                assert!(approx_eq(dram, 2000.0, TOLERANCE), "bad value {module}");
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

fn get_point<'a>(m: &'a Vec<&'a MeasurementPoint>, resource: Resource, sensor: &str) -> Option<&'a MeasurementPoint> {
    m.iter()
        .find(|p| {
            p.resource == resource
                && p.attributes()
                    .find(|(k, v)| *k == "sensor" && v.to_string() == sensor)
                    .is_some()
        })
        .map(|p| &**p)
}
