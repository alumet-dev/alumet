mod probe;

use anyhow::Context;
use probe::GraceHopperProbe;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufRead},
    path::PathBuf,
    time::Duration,
};

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};

pub struct GraceHopperPlugin {
    config: Config,
}

impl AlumetPlugin for GraceHopperPlugin {
    fn name() -> &'static str {
        "grace-hopper"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(GraceHopperPlugin { config }))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let base_dir = self.config.root_path.to_string();
        // Try to open the directory
        if let Ok(entries) = fs::read_dir(base_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    // Check if it's a directory
                    if path.is_dir() {
                        let device_file = path.join("device").join("power1_oem_info");
                        // Check if file "power1_oem_info" exist
                        if device_file.exists() {
                            let (sensor, socket) = parse_sensor_information(device_file.clone())?;
                            let source = Box::new(GraceHopperProbe::new(
                                alumet,
                                socket.clone(),
                                sensor.clone(),
                                device_file.clone().parent(),
                            )?);
                            let name = format!("{}_{}", sensor.clone(), socket.clone());
                            alumet.add_source(
                                name.as_str(),
                                source,
                                TriggerSpec::at_interval(self.config.poll_interval),
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn parse_sensor_information(device_file: PathBuf) -> Result<(String, String), anyhow::Error> {
    let file = File::open(&device_file).context("Failed to open the file")?;
    let reader = io::BufReader::new(file);
    for line in reader.lines() {
        let line = line.context("Failed to read the line from file")?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let sensor = parts[0].to_string();
            let socket = parts[3].to_string();
            return Ok((sensor, socket));
        }
    }
    // Return an error if no valid line found
    Err(anyhow::anyhow!(
        "Can't parse the content of the file: {:?}",
        device_file
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Initial interval between two Nvidia measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// Path to check hwmon.
    root_path: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1 Hz
            root_path: "/sys/class/hwmon".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraceHopperPlugin;
    use alumet::agent;
    use alumet::agent::plugin::PluginSet;
    use alumet::measurement::WrappedMeasurementValue;
    use alumet::pipeline::naming::SourceName;
    use alumet::plugin::PluginMetadata;
    use alumet::test::{RuntimeExpectations, StartupExpectations};
    use alumet::units::PrefixedUnit;
    use anyhow::Result;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    const TIMEOUT: Duration = Duration::from_secs(5);

    #[test]
    fn test_parse_sensor_information() {
        let test_cases = vec![
            ("Module Power Socket 2", "Module", "2"),
            ("Grace Power Socket 2", "Grace", "2"),
            ("CPU Power Socket 2", "CPU", "2"),
            ("SysIO Power Socket 2", "SysIO", "2"),
            ("Module Power Socket 3", "Module", "3"),
            ("Grace Power Socket 3", "Grace", "3"),
            ("CPU Power Socket 3", "CPU", "3"),
            ("SysIO Power Socket 3", "SysIO", "3"),
            ("Module Power Socket 0", "Module", "0"),
            ("Grace Power Socket 0", "Grace", "0"),
            ("CPU Power Socket 0", "CPU", "0"),
            ("SysIO Power Socket 0", "SysIO", "0"),
            ("Module Power Socket 1", "Module", "1"),
            ("Grace Power Socket 1", "Grace", "1"),
            ("CPU Power Socket 1", "CPU", "1"),
            ("SysIO Power Socket 1", "SysIO", "1"),
        ];

        for (line, expected_sensor, expected_socket) in test_cases {
            let root = tempdir().unwrap();
            let file_path = root.path().join("power1_oem");
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "{}", line).unwrap();
            let result = parse_sensor_information(file_path);
            assert!(result.is_ok(), "Expected Ok for input '{}'", line);
            let (sensor, socket) = result.unwrap();
            // Check content
            assert_eq!(sensor, expected_sensor, "Incorrect sensor for input '{}'", line);
            assert_eq!(socket, expected_socket, "Incorrect socket for input '{}'", line);
        }
    }

    fn fake_grace_hopper_plugin() -> GraceHopperPlugin {
        GraceHopperPlugin {
            config: Config {
                poll_interval: Duration::from_secs(1),
                root_path: String::from("/sys/class/hwmon"),
            },
        }
    }

    // Test `default_config` function of grace-hopper plugin
    #[test]
    fn test_default_config() {
        let result = GraceHopperPlugin::default_config().unwrap();
        assert!(result.is_some(), "result = None");

        let config_table = result.unwrap();
        let config: Config = deserialize_config(config_table).expect("Failed to deserialize config");

        assert_eq!(config.root_path, "/sys/class/hwmon".to_string());
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    #[test]
    fn test_init() -> Result<()> {
        let config_table = serialize_config(Config::default())?;
        let plugin = GraceHopperPlugin::init(config_table)?;
        assert_eq!(plugin.config.poll_interval, Duration::from_secs(1));
        assert_eq!(plugin.config.root_path, String::from("/sys/class/hwmon"));
        Ok(())
    }

    // Test `stop` function to stop k8s plugin
    #[test]
    fn test_stop() {
        let mut plugin = fake_grace_hopper_plugin();
        let result = plugin.stop();
        assert!(result.is_ok(), "Stop should complete without errors.");
    }

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
        std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();

        let mut file = File::create(&file_path_info).unwrap();
        let mut _file_avg = File::create(&file_path_average).unwrap();
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
        std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
        let mut file = File::create(&file_path_info).unwrap();
        let mut file_avg = File::create(&file_path_average).unwrap();
        writeln!(file, "Module Power Socket 0").unwrap();
        writeln!(file_avg, "123456789").unwrap();

        let file_path_info = root.path().join("hwmon2/device/power1_oem_info");
        let file_path_average = root.path().join("hwmon2/device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
        let mut file = File::create(&file_path_info).unwrap();
        let mut file_avg = File::create(&file_path_average).unwrap();
        writeln!(file, "Grace Power Socket 0").unwrap();
        writeln!(file_avg, "987654321").unwrap();

        let file_path_info = root.path().join("hwmon3/device/power1_oem_info");
        let file_path_average = root.path().join("hwmon3/device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
        let mut file = File::create(&file_path_info).unwrap();
        let mut file_avg = File::create(&file_path_average).unwrap();
        writeln!(file, "CPU Power Socket 2").unwrap();
        writeln!(file_avg, "1234598761").unwrap();

        let file_path_info = root.path().join("hwmon6/device/power1_oem_info");
        let file_path_average = root.path().join("hwmon6/device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap()).unwrap();
        let mut file = File::create(&file_path_info).unwrap();
        let mut file_avg = File::create(&file_path_average).unwrap();
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
}
