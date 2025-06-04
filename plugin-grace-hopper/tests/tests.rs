use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::WrappedMeasurementValue,
    pipeline::naming::SourceName,
    plugin::PluginMetadata,
    test::{RuntimeExpectations, StartupExpectations},
    units::PrefixedUnit,
};
use plugin_grace_hopper::{Config, GraceHopperPlugin};
use std::io::Write;
use std::{fs::File, time::Duration};
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn test_correct_plugin_with_no_data() {
    let root = tempdir().unwrap();
    let root_path = root.path().to_str().unwrap().to_string();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path: root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new();

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .build_and_start()
        .unwrap();

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
    return;
}

#[test]
fn test_correct_plugin_init_with_one_source_empty_value() {
    let root = tempdir().unwrap();

    let root_path = root.path().to_str().unwrap().to_string();
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();

    let mut file = File::create(&file_path_info).unwrap();
    let mut _file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Module Power Socket 0").unwrap();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path: root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
        .expect_metric::<u64>("consumption", PrefixedUnit::micro(alumet::units::Unit::Watt))
        .expect_source("grace-hopper", "Module_0");

    let runtime_expectation = RuntimeExpectations::new().test_source(
        SourceName::from_str("grace-hopper", "Module_0"),
        || {},
        |m| {
            assert_eq!(m.len(), 1);
            for elm in m {
                assert!(elm.value == WrappedMeasurementValue::U64(0));
            }
        },
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
    return;
}

#[test]
fn test_correct_plugin_init_with_several_sources() {
    let root = tempdir().unwrap();

    let root_path = root.path().to_str().unwrap().to_string();
    let file_path_info = root.path().join("hwmon1/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon1/device/power1_average");
    let file_path_interval = root.path().join("hwmon1/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Module Power Socket 0").unwrap();
    writeln!(file_avg, "123456789").unwrap();

    let file_path_info = root.path().join("hwmon2/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon2/device/power1_average");
    let file_path_interval = root.path().join("hwmon2/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "Grace Power Socket 0").unwrap();
    writeln!(file_avg, "987654321").unwrap();

    let file_path_info = root.path().join("hwmon3/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon3/device/power1_average");
    let file_path_interval = root.path().join("hwmon3/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "CPU Power Socket 2").unwrap();
    writeln!(file_avg, "1234598761").unwrap();

    let file_path_info = root.path().join("hwmon6/device/power1_oem_info");
    let file_path_average = root.path().join("hwmon6/device/power1_average");
    let file_path_interval = root.path().join("hwmon6/device/power1_average_interval");
    std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
    let mut file = File::create(&file_path_info).unwrap();
    let mut file_avg = File::create(&file_path_average).unwrap();
    let mut _file_int = File::create(&file_path_interval).unwrap();
    writeln!(file, "SysIO Power Socket 2").unwrap();
    writeln!(file_avg, "678954321").unwrap();

    let mut plugins = PluginSet::new();
    let config = Config {
        poll_interval: Duration::from_secs(1),
        root_path: root_path,
    };

    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<GraceHopperPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let startup_expectation = StartupExpectations::new()
        .expect_metric::<u64>("consumption", PrefixedUnit::micro(alumet::units::Unit::Watt))
        .expect_source("grace-hopper", "Module_0")
        .expect_source("grace-hopper", "Grace_0")
        .expect_source("grace-hopper", "CPU_2")
        .expect_source("grace-hopper", "SysIO_2");

    let runtime_expectation = RuntimeExpectations::new()
        .test_source(
            SourceName::from_str("grace-hopper", "Module_0"),
            || {},
            |m| {
                assert_eq!(m.len(), 1);
                for elm in m {
                    assert!(elm.value == WrappedMeasurementValue::U64(123456789));
                }
            },
        )
        .test_source(
            SourceName::from_str("grace-hopper", "Grace_0"),
            || {},
            |m| {
                assert_eq!(m.len(), 1);
                for elm in m {
                    assert!(elm.value == WrappedMeasurementValue::U64(987654321));
                }
            },
        )
        .test_source(
            SourceName::from_str("grace-hopper", "CPU_2"),
            || {},
            |m| {
                assert_eq!(m.len(), 1);
                for elm in m {
                    assert!(elm.value == WrappedMeasurementValue::U64(1234598761));
                }
            },
        )
        .test_source(
            SourceName::from_str("grace-hopper", "SysIO_2"),
            || {},
            |m| {
                assert_eq!(m.len(), 1);
                for elm in m {
                    assert!(elm.value == WrappedMeasurementValue::U64(678954321));
                }
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectation)
        .with_expectations(runtime_expectation)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();

    return;
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}
