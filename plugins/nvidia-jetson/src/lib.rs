mod ina;
mod source;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

pub struct JetsonPlugin {
    config: Config,
}

impl JetsonPlugin {
    #[cfg(test)]
    fn sysfs_paths(&self) -> ina::InaSysfsPath<'_> {
        ina::InaSysfsPath {
            sysfs_ina_modern: &self.config.sysfs_ina_modern,
            sysfs_ina_old: &self.config.sysfs_ina_old,
        }
    }

    #[cfg(not(test))]
    fn sysfs_paths(&self) -> ina::InaSysfsPath<'_> {
        ina::InaSysfsPath::default()
    }
}

impl AlumetPlugin for JetsonPlugin {
    fn name() -> &'static str {
        "jetson"
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
        Ok(Box::new(JetsonPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let (mut sensors, errs) = ina::detect_ina_sensors(self.sysfs_paths())
            .context("no INA-3221 sensor found, are you running on a Jetson device?")?;
        ina::sort_sensors_recursively(&mut sensors);

        // print errors to help the admin
        if !errs.is_empty() {
            let mut msg = String::from("Some errors happened during the detection of INA-3221 sensors:");
            for err in errs {
                msg.push_str(&format!("\n- {err:#}"));
            }
            log::warn!("{msg}");
        }

        // ensure that there is at least one sensor with at least one valid channel
        let n_channels: usize = sensors.iter().map(|s| s.channels.len()).sum();
        if n_channels == 0 {
            return Err(anyhow::Error::msg(
                "no INA-3221 channel could be read, are the permissions set properly?",
            ));
        }

        // print valid sensors
        for sensor in &sensors {
            log::info!("Found INA-3221 sensor {}", sensor.metadata);
            for chan in &sensor.channels {
                let id = chan.id;
                let label = chan.label.as_deref().unwrap_or("?");
                log::debug!("  - channel {id}: {label}");
            }
        }

        // prepare the measurement source
        let source = source::JetsonInaSource::open_sensors(sensors, alumet)?;
        let trigger = TriggerSpec::builder(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .build()?;
        alumet.add_source("builtin_ina_sensor", Box::new(source), trigger)?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Initial interval between two measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// Initial interval between two measurement flushes.
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,

    #[cfg(test)]
    sysfs_ina_modern: String,

    #[cfg(test)]
    sysfs_ina_old: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            #[cfg(test)]
            sysfs_ina_modern: ina::modern::SYSFS_INA_MODERN.to_string(),
            #[cfg(test)]
            sysfs_ina_old: ina::old::SYSFS_INA_OLD.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        fs::Permissions,
        os::unix::fs::PermissionsExt,
    };

    use alumet::{
        agent::plugin::{PluginInfo, PluginSet},
        measurement::{AttributeValue, MeasurementPoint},
        metrics::RawMetricId,
        pipeline::naming::SourceName,
        plugin::PluginMetadata,
        test::{RuntimeExpectations, StartupExpectations},
        units::{PrefixedUnit, Unit},
    };
    use tempfile::tempdir;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(1);

    #[test]
    fn test_plugin_with_modern_sysfs() {
        let tmp = tempdir().unwrap();

        // Create the fake sensor directories
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");
        let hwmon0 = root.join("1-0040/hwmon/hwmon0");
        let hwmon1 = root.join("1-0041/hwmon/hwmon1");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::create_dir_all(&hwmon1).unwrap();

        // Create the files that contains the label and metrics
        std::fs::write(hwmon0.join("in0_label"), "Sensor 0, channel 0").unwrap();
        std::fs::write(hwmon0.join("curr0_input"), "0").unwrap();
        std::fs::write(hwmon0.join("in0_input"), "1").unwrap();

        std::fs::write(hwmon0.join("in1_label"), "Sensor 0, channel 1").unwrap();
        std::fs::write(hwmon0.join("curr1_input"), "10").unwrap();
        std::fs::write(hwmon0.join("in1_input"), "11").unwrap();

        std::fs::write(hwmon1.join("in0_label"), "Sensor 1, channel 0").unwrap();
        std::fs::write(hwmon1.join("curr0_input"), "100").unwrap();
        std::fs::write(hwmon1.join("in0_input"), "101").unwrap();

        // Create the config
        let sysfs_root = root.to_str().unwrap();
        let config = toml::from_str(&format!(
            r#" 
                poll_interval = "1s"
                flush_interval = "1s"
                sysfs_ina_modern = "{sysfs_root}"
                sysfs_ina_old = ""
            "#
        ))
        .unwrap();

        // Start Alumet with the plugin.
        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<JetsonPlugin>(),
            enabled: true,
            config: Some(config),
        });

        let startup = StartupExpectations::new()
            .expect_metric::<u64>("input_current", PrefixedUnit::milli(Unit::Ampere))
            .expect_metric::<u64>("input_voltage", PrefixedUnit::milli(Unit::Volt))
            .expect_source("jetson", "builtin_ina_sensor");

        let runtime = RuntimeExpectations::new().test_source(
            SourceName::from_str("jetson", "builtin_ina_sensor"),
            || {},
            |out| {
                let out = out.measurements();
                println!("{out:?}");

                // group measurements by (i2c address, channel, metric) to check them later (the order in the buffer is not guaranteed)
                let mut measurements: HashMap<(u64, u64, RawMetricId), MeasurementPoint> = HashMap::new();
                for m in out {
                    let channel_id = m
                        .attributes()
                        .find(|attr| attr.0 == "ina_channel_id")
                        .expect("missing attribute ina_channel_id")
                        .1;
                    let channel_id = match channel_id {
                        AttributeValue::U64(id) => *id,
                        _ => panic!("channel_id should be a U64"),
                    };
                    let i2c_address = m
                        .attributes()
                        .find(|attr| attr.0 == "ina_i2c_address")
                        .expect("missing attribute ina_i2c_address")
                        .1;
                    let i2c_address = match i2c_address {
                        AttributeValue::U64(id) => *id,
                        _ => panic!("ina_i2c_address should be a U64"),
                    };

                    if measurements
                        .insert((i2c_address, channel_id, m.metric), m.to_owned())
                        .is_some()
                    {
                        panic!("only one measurement should be produced for each triple (sensor, channel, metric)")
                    }
                }

                // check the measurements
                // TODO check the values by using the metric id… but there isn't an easy way to access the metric registry from here.
                let sensors_and_channels: HashSet<(u64, u64)> =
                    measurements.into_keys().map(|(s, c, _)| (s, c)).collect();
                assert_eq!(
                    sensors_and_channels,
                    HashSet::from_iter([(0x40, 0), (0x40, 1), (0x41, 0)])
                );
            },
        );

        let agent = alumet::agent::Builder::new(plugins)
            .with_expectations(startup)
            .with_expectations(runtime)
            .build_and_start()
            .expect("agent should start");
        agent.wait_for_shutdown(TIMEOUT).expect("pipeline should run fine");
    }

    #[test]
    fn bad_permissions_1() {
        let tmp = tempdir().unwrap();

        // Create the fake sysfs hierarchy
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");
        let hwmon6 = root.join("1-0040/hwmon/hwmon6");
        std::fs::create_dir_all(&hwmon6).unwrap();
        std::fs::write(hwmon6.join("in0_label"), "Sensor 0, channel 0").unwrap();
        std::fs::write(hwmon6.join("curr0_input"), "0").unwrap();
        std::fs::write(hwmon6.join("in0_input"), "1").unwrap();

        // Make the channel files unreadable
        std::fs::set_permissions(hwmon6.join("in0_label"), Permissions::from_mode(0)).unwrap();
        std::fs::set_permissions(hwmon6.join("curr0_input"), Permissions::from_mode(0)).unwrap();
        std::fs::set_permissions(hwmon6.join("in0_input"), Permissions::from_mode(0)).unwrap();

        // Create the config
        let sysfs_root = root.to_str().unwrap();
        let config = toml::from_str(&format!(
            r#"
                poll_interval = "1s"
                flush_interval = "1s"
                sysfs_ina_modern = "{sysfs_root}"
                sysfs_ina_old = ""
            "#
        ))
        .unwrap();

        // Start Alumet with the plugin.
        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<JetsonPlugin>(),
            enabled: true,
            config: Some(config),
        });

        let agent = alumet::agent::Builder::new(plugins).build_and_start();
        assert!(agent.is_err(), "plugin should not start");
    }

    #[test]
    fn bad_permissions_2() {
        let tmp = tempdir().unwrap();

        // Create the fake sysfs hierarchy
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");
        let hwmon6 = root.join("1-0040/hwmon/hwmon6");
        std::fs::create_dir_all(&hwmon6).unwrap();
        std::fs::write(hwmon6.join("in0_label"), "Sensor 0, channel 0").unwrap();
        std::fs::write(hwmon6.join("curr0_input"), "0").unwrap();
        std::fs::write(hwmon6.join("in0_input"), "1").unwrap();

        // Make the hwmon dir unreadable
        std::fs::set_permissions(hwmon6, Permissions::from_mode(0)).unwrap();

        // Create the config
        let sysfs_root = root.to_str().unwrap();
        let config = toml::from_str(&format!(
            r#"
                poll_interval = "1s"
                flush_interval = "1s"
                sysfs_ina_modern = "{sysfs_root}"
                sysfs_ina_old = ""
            "#
        ))
        .unwrap();

        // Start Alumet with the plugin.
        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<JetsonPlugin>(),
            enabled: true,
            config: Some(config),
        });

        let agent = alumet::agent::Builder::new(plugins).build_and_start();
        assert!(agent.is_err(), "plugin should not start");
    }

    #[test]
    fn bad_permissions_3() {
        let tmp = tempdir().unwrap();

        // Create the fake sysfs root, but unreadable
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, Permissions::from_mode(0)).unwrap();

        // Create the config
        let sysfs_root = root.to_str().unwrap();
        let config = toml::from_str(&format!(
            r#"
                poll_interval = "1s"
                flush_interval = "1s"
                sysfs_ina_modern = "{sysfs_root}"
                sysfs_ina_old = ""
            "#
        ))
        .unwrap();

        // Start Alumet with the plugin.
        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<JetsonPlugin>(),
            enabled: true,
            config: Some(config),
        });

        let agent = alumet::agent::Builder::new(plugins).build_and_start();
        assert!(agent.is_err(), "plugin should not start");
    }
}
