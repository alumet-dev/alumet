use std::{
    fs::ReadDir,
    path::{Path, PathBuf},
};

use alumet::units::PrefixedUnit;
use anyhow::Context;
use rustc_hash::FxHashMap;

use modern::ModernInaExplorer;
use old::OldInaExplorer;
use serde::{Deserialize, Serialize};

mod common;
pub mod modern;
pub mod old;

/// Detected INA sensor.
#[derive(Debug, PartialEq)]
pub struct InaSensor {
    /// Information about the device in the I2C sysfs.
    pub metadata: InaDeviceMetadata,
    /// Channels available on this sensor.
    /// Each INA3221 has at least one channel.
    pub channels: Vec<InaChannel>,
}

/// Detected INA channel.
#[derive(Debug, PartialEq)]
pub struct InaChannel {
    pub id: u32,
    pub label: Option<String>,
    pub metrics: Vec<InaRailMetric>,
}

/// Detected metric available in a channel.
#[derive(Debug, PartialEq)]
pub struct InaRailMetric {
    pub path: PathBuf,
    pub unit: PrefixedUnit,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InaDeviceMetadata {
    pub path: PathBuf,
    pub i2c_address: u32,
    pub number: u32,
}

/// Explores the sysfs to find the INA-3221 channels that are relevant for our measurement purposes.
pub trait InaExplorer {
    fn sysfs_root(&self) -> &Path;
    fn devices(&self) -> anyhow::Result<Vec<InaDeviceMetadata>>;
    fn analyze_entry(&self, channel_entry_path: &Path) -> anyhow::Result<EntryAnalysis>;
}

pub enum EntryAnalysis {
    /// This entry must be ignored.
    Ignore,
    /// This entry contains the label of the current channel.
    Label { channel_id: u32, label: String },
    /// This entry is sysfs node that provides measurements.
    MeasurementNode {
        channel_id: u32,
        unit: PrefixedUnit,
        metric_name: String,
        // TODO description
    },
}

/// Sorts a list of sensors and sorts each element in the sensors, recursively.
pub fn sort_sensors_recursively(sensors: &mut Vec<InaSensor>) {
    for s in sensors.iter_mut() {
        for chan in &mut s.channels {
            chan.metrics.sort_by_key(|m| m.name.clone());
        }
        s.channels.sort_by_key(|chan| chan.id);
    }
    sensors.sort_by_key(|s| s.metadata.i2c_address.clone());
}

impl std::fmt::Display for InaDeviceMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "device {} (i2c {}) at {}",
            self.number,
            self.i2c_address,
            self.path.display()
        )
    }
}

impl InaChannel {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            label: None,
            metrics: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InaSysfsPath<'a> {
    pub sysfs_ina_modern: &'a str,
    pub sysfs_ina_old: &'a str,
}

impl Default for InaSysfsPath<'_> {
    fn default() -> Self {
        Self {
            sysfs_ina_modern: modern::SYSFS_INA_MODERN,
            sysfs_ina_old: old::SYSFS_INA_OLD,
        }
    }
}

/// Returns a list of all the INA sensors available on the machine.
///
/// This function supports multiple version of the NVIDIA Jetpack SDK.
pub fn detect_ina_sensors(paths: InaSysfsPath) -> anyhow::Result<(Vec<InaSensor>, Vec<anyhow::Error>)> {
    if let Ok(true) = std::fs::exists(paths.sysfs_ina_modern) {
        explore_ina_devices(ModernInaExplorer::new(paths.sysfs_ina_modern))
    } else if let Ok(true) = std::fs::exists(paths.sysfs_ina_old) {
        explore_ina_devices(OldInaExplorer::new(paths.sysfs_ina_old))
    } else {
        Err(anyhow::Error::msg(format!(
            "no INA-3221 sensor detected: neither {} nor {} exist",
            paths.sysfs_ina_modern, paths.sysfs_ina_old
        )))
    }
}

pub fn explore_ina_devices(explorer: impl InaExplorer) -> anyhow::Result<(Vec<InaSensor>, Vec<anyhow::Error>)> {
    let mut sensors = Vec::new();
    let mut errors = Vec::new();

    for device in explorer.devices()? {
        // resolve symlinks now
        let canonical_path = device
            .path
            .canonicalize()
            .with_context(|| format!("failed to canonicalize {:?}", device.path))?;

        match std::fs::read_dir(&canonical_path) {
            Ok(ls) => {
                let channels = detect_device_channels(&explorer, ls, &mut errors);
                let sensor = InaSensor {
                    metadata: device,
                    channels,
                };
                sensors.push(sensor);
            }
            Err(e) => errors
                .push(anyhow::Error::from(e).context(format!("could not list the content of {:?}", &canonical_path))),
        }
    }
    Ok((sensors, errors))
}

/// Detects the channels of an INA I2C device.
fn detect_device_channels(
    explorer: &impl InaExplorer,
    ls: ReadDir,
    errors: &mut Vec<anyhow::Error>,
) -> Vec<InaChannel> {
    // The label of the channel is provided by a dedicated file, but we are not
    // guaranteed to read it first (the order of the files in ReadDir is unspecified). Therefore, we fill the fields little by little.
    let mut channels_by_id = FxHashMap::default();

    for entry in ls.into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        match explorer.analyze_entry(&entry_path) {
            Err(err) => {
                errors.push(err.context(format!("failed to analyze I2C entry {entry_path:?}")));
            }
            Ok(EntryAnalysis::Ignore) => (),
            Ok(EntryAnalysis::Label { channel_id, label }) => {
                channels_by_id
                    .entry(channel_id)
                    .or_insert_with_key(|id| InaChannel::new(*id))
                    .label = Some(label);
            }
            Ok(EntryAnalysis::MeasurementNode {
                channel_id,
                unit,
                metric_name: name,
            }) => {
                let metric = InaRailMetric {
                    path: entry_path,
                    unit,
                    name,
                };
                channels_by_id
                    .entry(channel_id)
                    .or_insert_with_key(|id| InaChannel::new(*id))
                    .metrics
                    .push(metric);
            }
        }
    }

    channels_by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alumet::units::{PrefixedUnit, Unit};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use crate::ina::{
        common::{METRIC_CURRENT, METRIC_POWER, METRIC_VOLTAGE},
        modern::ModernInaExplorer,
        old::OldInaExplorer,
        InaDeviceMetadata,
    };

    use super::{explore_ina_devices, sort_sensors_recursively, InaChannel, InaRailMetric, InaSensor};

    #[test]
    fn ina_modern() {
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
        std::fs::write(hwmon0.join("curr0_crit"), "2").unwrap();
        std::fs::write(hwmon0.join("crit0_max"), "3").unwrap();

        std::fs::write(hwmon0.join("in1_label"), "Sensor 0, channel 1").unwrap();
        std::fs::write(hwmon0.join("curr1_input"), "10").unwrap();
        std::fs::write(hwmon0.join("in1_input"), "11").unwrap();
        std::fs::write(hwmon0.join("curr1_crit"), "12").unwrap();
        std::fs::write(hwmon0.join("crit1_max"), "13").unwrap();

        std::fs::write(hwmon1.join("in0_label"), "Sensor 1, channel 0").unwrap();
        std::fs::write(hwmon1.join("curr0_input"), "100").unwrap();
        std::fs::write(hwmon1.join("in0_input"), "101").unwrap();
        std::fs::write(hwmon1.join("curr0_crit"), "102").unwrap();
        std::fs::write(hwmon1.join("crit0_max"), "103").unwrap();

        // Test the detection
        let (sensors, errs) = explore_ina_devices(ModernInaExplorer::new(root)).expect("detection failed");
        assert!(errs.is_empty(), "detection failed");
        let mut sensor_addrs: Vec<u32> = sensors.iter().map(|s| s.metadata.i2c_address).collect();
        sensor_addrs.sort();
        assert_eq!(sensor_addrs, vec![0x40, 0x41]);

        let expected_channel_labels: HashMap<u32, Vec<&str>> = HashMap::from_iter(vec![
            (0x40, vec!["Sensor 0, channel 0", "Sensor 0, channel 1"]),
            (0x41, vec!["Sensor 1, channel 0"]),
        ]);
        let mut expected_metrics = vec![METRIC_CURRENT, METRIC_VOLTAGE];
        expected_metrics.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<String> = sensor
                .channels
                .iter()
                .map(|chan| chan.label.to_owned().unwrap_or_else(|| format!("channel_{}", chan.id)))
                .collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[&sensor.metadata.i2c_address];
            assert_eq!(expected_labels, &channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();
                assert_eq!(metrics, expected_metrics);
            }
        }
    }

    #[test]
    fn ina_modern_some_errors() {
        let tmp = tempdir().unwrap();

        // Create the fake sensor directories
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");
        let hwmon0 = root.join("1-0040/hwmon/hwmon0");
        let hwmon1 = root.join("1-0041/hwmon/hwmon1");
        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::create_dir_all(&hwmon1).unwrap();

        // Create the files that contains the label and metrics
        std::fs::write(hwmon0.join("in0_label"), "Sensor 0, channel 0").unwrap();
        std::fs::write(hwmon0.join("curr0_input"), "").unwrap(); // INVALID
        std::fs::write(hwmon0.join("in0_input"), "1").unwrap();

        std::fs::write(hwmon0.join("in1_label"), "Sensor 0, channel 1").unwrap();
        std::fs::write(hwmon0.join("curr1_input"), "10").unwrap();
        std::fs::write(hwmon0.join("in1_input"), "11").unwrap();

        // no "in0_label"
        std::fs::write(hwmon1.join("curr0_input"), "100").unwrap();
        std::fs::write(hwmon1.join("in0_input"), "101").unwrap();
        std::fs::write(hwmon1.join("badname"), "101").unwrap();

        // Test the detection
        let (sensors, errs) = explore_ina_devices(ModernInaExplorer::new(root)).expect("detection failed");
        assert!(!errs.is_empty(), "detection should report some errors");
        for err in errs {
            println!("{err:#}");
        }

        let mut sensor_addrs: Vec<u32> = sensors.iter().map(|s| s.metadata.i2c_address).collect();
        sensor_addrs.sort();
        assert_eq!(sensor_addrs, vec![0x40, 0x41]);

        let expected_channel_labels: HashMap<u32, Vec<Option<String>>> = HashMap::from_iter(vec![
            (
                0x40,
                vec![
                    Some("Sensor 0, channel 0".to_string()),
                    Some("Sensor 0, channel 1".to_string()),
                ],
            ),
            (0x41, vec![None]),
        ]);

        let expected_metrics_0_0 = vec![METRIC_VOLTAGE];
        let mut expected_metrics_nominal = vec![METRIC_CURRENT, METRIC_VOLTAGE];
        expected_metrics_nominal.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<Option<String>> =
                sensor.channels.iter().map(|chan| chan.label.clone()).collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[&sensor.metadata.i2c_address];
            assert_eq!(expected_labels, &channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();

                if sensor.metadata.i2c_address == 0x40 && channel.id == 0 {
                    // Because of the error, `curr0_input` is not included in the list of available metrics.
                    assert_eq!(metrics, expected_metrics_0_0);
                } else {
                    assert_eq!(metrics, expected_metrics_nominal);
                }
            }
        }
    }

    #[test]
    fn ina_modern_symlinks() {
        let tmp = tempdir().unwrap();

        // Create the fake sensor directories
        let actual_root = tmp.path().join("ina_actual_root");
        let root = tmp.path().join("ina_modern_symlinks");
        println!("actual root: {actual_root:?}");
        println!("linked root: {root:?}");

        let sensor_0040_link = root.join("1-0040");
        let sensor_0041_link = root.join("1-0041");
        let sensor_0040 = actual_root.join("1-0040");
        let sensor_0041 = actual_root.join("1-0041");

        let hwmon0_link = root.join("1-0040/hwmon/hwmon0");
        let hwmon1_link = root.join("1-0041/hwmon/hwmon1");
        let hwmon0 = actual_root.join("1-0040/hwmon/hwmon0");
        let hwmon1 = actual_root.join("1-0041/hwmon/hwmon1");

        std::fs::create_dir_all(&hwmon0).unwrap();
        std::fs::create_dir_all(&hwmon1).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let _ = std::os::unix::fs::symlink(&sensor_0040, &sensor_0040_link)
            .inspect_err(|e| panic!("failed to create symlink {sensor_0040_link:?} -> {sensor_0040:?}: {e}"));
        let _ = std::os::unix::fs::symlink(&sensor_0041, &sensor_0041_link)
            .inspect_err(|e| panic!("failed to create symlink {sensor_0041_link:?} -> {sensor_0041:?}: {e}"));

        // Create the files that contains the label and metrics
        std::fs::write(hwmon0.join("in0_label"), "Sensor 0, channel 0").unwrap();
        std::fs::write(hwmon0.join("curr0_input"), "0").unwrap();
        std::fs::write(hwmon0.join("in0_input"), "1").unwrap();
        let expected_s0_chan0 = InaChannel {
            id: 0,
            label: Some(String::from("Sensor 0, channel 0")),
            metrics: vec![
                InaRailMetric {
                    path: hwmon0.join("curr0_input"),
                    unit: PrefixedUnit::milli(Unit::Ampere),
                    name: String::from(METRIC_CURRENT),
                },
                InaRailMetric {
                    path: hwmon0.join("in0_input"),
                    unit: PrefixedUnit::milli(Unit::Volt),
                    name: String::from(METRIC_VOLTAGE),
                },
            ],
        };

        std::fs::write(hwmon0.join("in1_label"), "Sensor 0, channel 1").unwrap();
        std::fs::write(hwmon0.join("curr1_input"), "10").unwrap();
        std::fs::write(hwmon0.join("in1_input"), "11").unwrap();
        std::fs::write(hwmon0.join("curr1_crit"), "12").unwrap();
        std::fs::write(hwmon0.join("crit1_max"), "13").unwrap();
        let expected_s0_chan1 = InaChannel {
            id: 1,
            label: Some(String::from("Sensor 0, channel 1")),
            metrics: vec![
                InaRailMetric {
                    path: hwmon0.join("curr1_input"),
                    unit: PrefixedUnit::milli(Unit::Ampere),
                    name: String::from(METRIC_CURRENT),
                },
                InaRailMetric {
                    path: hwmon0.join("in1_input"),
                    unit: PrefixedUnit::milli(Unit::Volt),
                    name: String::from(METRIC_VOLTAGE),
                },
            ],
        };

        std::fs::write(hwmon1.join("in0_label"), "Sensor 1, channel 0").unwrap();
        std::fs::write(hwmon1.join("curr0_input"), "100").unwrap();
        std::fs::write(hwmon1.join("in0_input"), "101").unwrap();
        std::fs::write(hwmon1.join("curr0_crit"), "102").unwrap();
        std::fs::write(hwmon1.join("curr0_max"), "103").unwrap();
        let expected_s1_chan0 = InaChannel {
            id: 0,
            label: Some(String::from("Sensor 1, channel 0")),
            metrics: vec![
                InaRailMetric {
                    path: hwmon1.join("curr0_input"),
                    unit: PrefixedUnit::milli(Unit::Ampere),
                    name: String::from(METRIC_CURRENT),
                },
                InaRailMetric {
                    path: hwmon1.join("in0_input"),
                    unit: PrefixedUnit::milli(Unit::Volt),
                    name: String::from(METRIC_VOLTAGE),
                },
            ],
        };

        // no label for channel 5
        std::fs::write(hwmon1.join("curr5_input"), "100").unwrap();
        std::fs::write(hwmon1.join("curr5_max"), "100").unwrap();
        std::fs::write(hwmon1.join("curr5_max_alarm"), "100").unwrap();
        std::fs::write(hwmon1.join("curr5_crit"), "100").unwrap();
        std::fs::write(hwmon1.join("curr5_crit_alarm"), "100").unwrap();
        std::fs::write(hwmon1.join("in5_input"), "100").unwrap();
        std::fs::write(hwmon1.join("in5_enable"), "100").unwrap();
        std::fs::write(hwmon1.join("shunt5_resistor"), "100").unwrap();
        // some files that don't contain any metric
        std::fs::write(hwmon1.join("power"), "123456789").unwrap();
        std::fs::write(hwmon1.join("uevent"), "").unwrap();
        std::fs::write(hwmon1.join("samples"), "").unwrap();
        let expected_s1_chan5 = InaChannel {
            id: 5,
            label: None,
            metrics: vec![
                InaRailMetric {
                    path: hwmon1.join("curr5_input"),
                    unit: PrefixedUnit::milli(Unit::Ampere),
                    name: String::from(METRIC_CURRENT),
                },
                InaRailMetric {
                    path: hwmon1.join("in5_input"),
                    unit: PrefixedUnit::milli(Unit::Volt),
                    name: String::from(METRIC_VOLTAGE),
                },
            ],
        };

        // Build what we expect
        let mut expected_sensors = vec![
            InaSensor {
                metadata: InaDeviceMetadata {
                    path: hwmon0_link,
                    i2c_address: 0x40,
                    number: 0,
                },
                channels: vec![expected_s0_chan0, expected_s0_chan1],
            },
            InaSensor {
                metadata: InaDeviceMetadata {
                    path: hwmon1_link,
                    i2c_address: 0x41,
                    number: 1,
                },
                channels: vec![expected_s1_chan0, expected_s1_chan5],
            },
        ];

        // Test the detection
        let (mut sensors, errs) = explore_ina_devices(ModernInaExplorer::new(root)).expect("detection failed");
        assert!(errs.is_empty(), "detection failed");
        sort_sensors_recursively(&mut expected_sensors);
        sort_sensors_recursively(&mut sensors);
        assert_eq!(expected_sensors, sensors);
    }

    #[test]
    fn ina_old() {
        let tmp = tempdir().unwrap();

        // Create the fake sensor directories
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-old");
        let device0 = root.join("1-0040/iio:device0");
        let device1 = root.join("1-0041/iio:device1");
        std::fs::create_dir_all(&device0).unwrap();
        std::fs::create_dir_all(&device1).unwrap();

        // Create the files that contains the label and metrics
        std::fs::write(device0.join("rail_name_0"), "Sensor 0, channel 0").unwrap();
        std::fs::write(device0.join("in_current0_input"), "0").unwrap();
        std::fs::write(device0.join("in_voltage0_input"), "1").unwrap();
        std::fs::write(device0.join("in_power0_input"), "2").unwrap();
        std::fs::write(device0.join("crit_current_limit_0"), "3").unwrap();
        std::fs::write(device0.join("warn_current_limit_0"), "4").unwrap();

        std::fs::write(device0.join("rail_name_1"), "Sensor 0, channel 1").unwrap();
        std::fs::write(device0.join("in_current1_input"), "10").unwrap();
        std::fs::write(device0.join("in_voltage1_input"), "11").unwrap();
        std::fs::write(device0.join("in_power1_input"), "12").unwrap();
        std::fs::write(device0.join("crit_current_limit_1"), "13").unwrap();
        std::fs::write(device0.join("warn_current_limit_1"), "14").unwrap();

        std::fs::write(device1.join("rail_name_0"), "Sensor 1, channel 0").unwrap();
        std::fs::write(device1.join("in_current0_input"), "100").unwrap();
        std::fs::write(device1.join("in_voltage0_input"), "101").unwrap();
        std::fs::write(device1.join("in_power0_input"), "102").unwrap();
        std::fs::write(device1.join("crit_current_limit_0"), "103").unwrap();
        std::fs::write(device1.join("warn_current_limit_0"), "104").unwrap();

        // Test the detection
        let (sensors, errs) = explore_ina_devices(OldInaExplorer::new(root)).expect("detection failed");
        assert!(errs.is_empty(), "detection failed");
        let mut sensor_ids: Vec<u32> = sensors.iter().map(|s| s.metadata.i2c_address).collect();
        sensor_ids.sort();
        assert_eq!(sensor_ids, vec![0x40, 0x41]);

        let expected_channel_labels: HashMap<u32, Vec<&str>> = HashMap::from_iter(vec![
            (0x40, vec!["Sensor 0, channel 0", "Sensor 0, channel 1"]),
            (0x41, vec!["Sensor 1, channel 0"]),
        ]);
        let mut expected_metrics = vec![METRIC_CURRENT, METRIC_VOLTAGE, METRIC_POWER];
        expected_metrics.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<String> = sensor
                .channels
                .iter()
                .map(|chan| chan.label.to_owned().unwrap_or_else(|| format!("channel_{}", chan.id)))
                .collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[&sensor.metadata.i2c_address];
            assert_eq!(expected_labels, &channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();

                assert_eq!(metrics, expected_metrics);
            }
        }
    }

    #[test]
    fn no_ina() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join(".i-do-not-exist");
        explore_ina_devices(ModernInaExplorer::new(&root)).expect_err("should fail");
        explore_ina_devices(OldInaExplorer::new(&root)).expect_err("should fail");
    }
}
