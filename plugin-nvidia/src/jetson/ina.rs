use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use alumet::units::{PrefixedUnit, Unit};
use anyhow::{anyhow, Context};
use regex::{Match, Regex};
use thiserror::Error;

/// Detected INA sensor.
#[derive(Debug)]
pub struct InaSensor {
    /// Path to the sysfs directory of the sensor.
    pub path: PathBuf,
    /// I2C id of the sensor.
    pub i2c_id: String,
    /// Channels available on this sensor.
    /// Each INA3221 has at least one channel.
    pub channels: Vec<InaChannel>,
}

/// Detected INA channel.
#[derive(Debug)]
pub struct InaChannel {
    pub id: u32,
    pub label: Option<String>,
    pub metrics: Vec<InaRailMetric>,
    // Added in a second pass based on the Jetson documentation. (TODO: fill it)
    pub description: Option<String>,
}

/// Detected metric available in a channel.
#[derive(Debug)]
pub struct InaRailMetric {
    pub path: PathBuf,
    pub unit: PrefixedUnit,
    pub name: String,
}

/// Returns a list of all the INA sensors available on the machine.
///
/// This function supports multiple version of the NVIDIA Jetpack SDK.
pub fn detect_ina_sensors() -> anyhow::Result<Vec<InaSensor>> {
    match detect_hierarchy_modern(SYSFS_INA) {
        Ok(res) => Ok(res),
        Err(DetectionError::SysfsAccess(err)) => {
            log::debug!("modern hierarchy detection failed, trying the old version: {err}");
            match detect_hierarchy_old_v4(SYSFS_INA_OLD) {
                Ok(res) => Ok(res),
                Err(DetectionError::SysfsAccess(err)) => {
                    log::error!("old hierarchy detection failed: {err}");
                    Ok(vec![])
                }
                Err(err) => Err(err.into()),
            }
        }
        Err(DetectionError::Internal(err)) => Err(err),
    }
}

#[derive(Debug, Error)]
enum DetectionError {
    /// Failed to access the root path that should point to INA sensors.
    ///
    /// If this error occurs, it probably means that the version of the hierarchy
    /// is not the expected one.
    #[error("could not access the sysfs: {0}")]
    SysfsAccess(anyhow::Error),
    /// Internal detection error.
    #[error("internal detection error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl From<std::io::Error> for DetectionError {
    fn from(value: std::io::Error) -> Self {
        DetectionError::Internal(value.into())
    }
}

const SYSFS_INA_OLD: &str = "/sys/bus/i2c/drivers/ina3221x";
const SYSFS_INA: &str = "/sys/bus/i2c/drivers/ina3221";

/// Tries to guess the measurement unit that corresponds to the given channel prefix.
/// Returns `None` if the unit cannot be found.
fn guess_channel_unit(prefix: &Option<Match>) -> Option<PrefixedUnit> {
    let prefix = prefix.unwrap().as_str();
    match prefix {
        "curr" => Some(PrefixedUnit::milli(Unit::Ampere)),
        "in" => Some(PrefixedUnit::milli(Unit::Volt)),
        "crit" => Some(PrefixedUnit::milli(Unit::Ampere)),
        _ if prefix.contains("volt") => Some(PrefixedUnit::milli(Unit::Volt)),
        _ if prefix.contains("current") => Some(PrefixedUnit::milli(Unit::Ampere)),
        _ if prefix.contains("power") => Some(PrefixedUnit::milli(Unit::Watt)),
        _ if prefix.contains("shunt") => Some(PrefixedUnit::milli(Unit::Custom {
            unique_name: "Ohm".to_string(),
            display_name: "Ω".to_string(),
        })),
        _ => None,
    }
}

/// Detect the available INA sensors, assuming that Nvidia Jetpack version >= 5.0 is installed.
///
/// The standard `sys_ina` looks like `/sys/bus/i2c/drivers/ina3221`.
fn detect_hierarchy_modern<P: AsRef<Path>>(sys_ina: P) -> Result<Vec<InaSensor>, DetectionError> {
    /// Look for a path of the form <sensor_path>/hwmon/hwmon<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        let hwmon = sensor_path.join("hwmon");
        for child in std::fs::read_dir(&hwmon)
            .with_context(|| format!("failed to list content of directory {}", hwmon.display()))?
        {
            let child = child?;
            let path = child.path();
            if path.file_name().unwrap().to_string_lossy().starts_with("hwmon") {
                log::trace!("Found sensor channel dir: {path:?}");
                return Ok(path);
            }
        }
        Err(anyhow!("no sensor found in {}", sensor_path.display()))
    }

    fn is_label_file(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool> {
        let prefix = prefix.context("parsing failed: missing prefix")?.as_str();
        let suffix = suffix.context("parsing failed: missing suffix")?.as_str();
        Ok(prefix == "in" && suffix == "label")
    }

    fn format_metric_name(prefix: &Option<Match>, suffix: &Option<Match>) -> String {
        format!("{}_{}", prefix.unwrap().as_str(), suffix.unwrap().as_str())
    }

    let metric_filename_pattern =
        Regex::new(r"(?<prefix>[a-zA-Z]+)(?<id>\d+)_(?<suffix>[a-zA-Z]+)").expect("regex should be valid");

    detect_hierarchy(
        sys_ina,
        metric_filename_pattern,
        sensor_channels_dir,
        guess_channel_unit,
        is_label_file,
        format_metric_name,
    )
}

/// Detect the available INA sensors, assuming that Nvidia Jetpack version 4.x is installed.
/// The hierarchy is subtly different in that case, and the metric and label files have different names than in v5+.
///
/// The standard `sys_ina` looks like `/sys/bus/i2c/drivers/ina3221x`
fn detect_hierarchy_old_v4<P: AsRef<Path>>(sys_ina: P) -> Result<Vec<InaSensor>, DetectionError> {
    /// Look for a path of the form <sensor_path>/iio:device<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        for child in std::fs::read_dir(sensor_path)? {
            let child = child?;
            let path = child.path();
            if path.file_name().unwrap().to_string_lossy().starts_with("iio:device") {
                log::trace!("Found sensor channel dir: {path:?}");
                return Ok(path);
            }
        }
        Err(anyhow!("no sensor found in {}", sensor_path.display()))
    }

    fn is_label_file(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool> {
        let prefix = prefix.context("parsing failed: missing prefix")?.as_str();
        Ok(prefix == "rail_name" && suffix.is_none())
    }

    fn format_metric_name(prefix: &Option<Match>, suffix: &Option<Match>) -> String {
        let prefix = prefix.unwrap().as_str();
        match suffix {
            Some(suffix_match) => format!("{prefix}{}", suffix_match.as_str()),
            None => prefix.to_string(),
        }
    }

    let metric_filename_pattern =
        Regex::new(r"(?<prefix>[a-zA-Z_]+?)_?(?<id>\d+)(?<suffix>_([a-zA-Z]+))?").expect("regex should be valid");

    detect_hierarchy(
        sys_ina,
        metric_filename_pattern,
        sensor_channels_dir,
        guess_channel_unit,
        is_label_file,
        format_metric_name,
    )
}

/// Detection function, common to all Jetpack versions.
fn detect_hierarchy<P: AsRef<Path>>(
    sys_ina: P,
    metric_filename_pattern: Regex,
    sensor_channels_dir: fn(sensor_path: &Path) -> anyhow::Result<PathBuf>,
    guess_channel_unit: fn(prefix: &Option<Match>) -> Option<PrefixedUnit>,
    is_label_file: fn(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool>,
    format_metric_name: fn(prefix: &Option<Match>, suffix: &Option<Match>) -> String,
) -> Result<Vec<InaSensor>, DetectionError> {
    // Look for channels and metrics.
    // - `channels_dir`: path of the form <sensor_path>/hwmon/hwmon<id>
    let sensor_channels = |channels_dir: &Path| -> anyhow::Result<Vec<InaChannel>> {
        let mut channel_metrics = HashMap::with_capacity(2);
        let mut channel_labels = HashMap::with_capacity(2);
        let entries =
            std::fs::read_dir(channels_dir).with_context(|| format!("failed to list content of {channels_dir:?}"))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("failed to get dir entry in {channels_dir:?}"))?;
            let path = entry.path();
            let filename = path.file_name().unwrap().to_str().unwrap().to_owned();
            let captures = metric_filename_pattern.captures(&filename);
            log::trace!("regex match for {filename}: {captures:?}");
            if let Some(groups) = captures {
                // Extract the prefix, suffix and channel id.
                let (prefix, suffix) = (&groups.name("prefix"), &groups.name("suffix"));
                let channel_id: u32 = groups["id"]
                    .parse()
                    .with_context(|| format!("invalid channel id: {}", &groups["id"]))?;

                // Determine whether the file contains the label of the channel, or a metric.
                let is_label = is_label_file(prefix, suffix)
                    .with_context(|| format!("failed to parse filename of INA metric: {}", filename))?;

                if is_label {
                    // This file contains the label of the channel.
                    let label = std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read channel label from {path:?}"))?;
                    let label = label.trim_end().to_owned();
                    channel_labels.insert(channel_id, label);
                } else {
                    // This file contains the (automatically updated) value of a metric.
                    match guess_channel_unit(prefix) {
                        Some(unit) => channel_metrics
                            .entry(channel_id)
                            .or_insert_with(|| Vec::with_capacity(5))
                            .push(InaRailMetric {
                                path,
                                unit,
                                name: format_metric_name(prefix, suffix),
                            }),
                        None => log::warn!(
                            "Could not determine the unit of this INA3221 channel: {}",
                            path.display()
                        ),
                    }
                }
            }
        }

        let res = channel_metrics
            .into_iter()
            .map(|(id, metrics)| InaChannel {
                id,
                label: channel_labels.get(&id).map(|v| v.to_owned()),
                metrics,
                description: None, // added later
            })
            .collect();
        Ok(res)
    };

    let dir_path: &Path = sys_ina.as_ref();
    log::trace!("Exploring {dir_path:?}");
    match std::fs::read_dir(dir_path) {
        Ok(dir) => {
            let mut sensors = Vec::new();
            for entry in dir {
                let entry = entry.map_err(|e| DetectionError::SysfsAccess(e.into()))?;
                // Resolve symbolic links.
                let resolved = entry
                    .path()
                    .canonicalize()
                    .with_context(|| format!("failed to canonicalize {:?}", entry.path()))?;
                let metadata = resolved
                    .metadata()
                    .with_context(|| format!("failed to get metadata of {:?}", resolved))?;
                if metadata.is_dir() && entry.file_name() != "module" {
                    log::trace!("Collecting sensors from dir {:?}", entry.path());
                    // Each subdirectory corresponds to one INA 3221 sensor.
                    let path = &resolved;
                    // The name of the directory corresponds to the i2c id.
                    let i2c_id = path.file_name().unwrap().to_str().unwrap().to_owned();
                    // Discover all the sensor channels (with their metrics).
                    match sensor_channels_dir(path).and_then(|dir| sensor_channels(&dir)) {
                        Ok(channels) => {
                            sensors.push(InaSensor {
                                path: path.clone(),
                                channels,
                                i2c_id,
                            });
                        }
                        Err(err) => {
                            log::error!("Sensor discovery failed for directory {}: {err:?}", resolved.display());
                        }
                    }
                }
            }
            log::debug!("Found sensors: {sensors:?}");
            Ok(sensors)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // The directory does not exist, simply return an empty list of sensors.
            Err(DetectionError::SysfsAccess(
                anyhow!(e).context(format!("{} does not exist", dir_path.display())),
            ))
        }
        Err(e) => Err(DetectionError::SysfsAccess(
            anyhow!(e).context(format!("failed to list the content of {}", dir_path.display())),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::jetson::ina::DetectionError;

    use super::{detect_hierarchy_modern, detect_hierarchy_old_v4};
    use tempfile::tempdir;

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
        let sensors = detect_hierarchy_modern(root).expect("detection failed");
        let mut sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        sensor_ids.sort();
        assert_eq!(sensor_ids, vec!["1-0040", "1-0041"]);

        let expected_channel_labels: HashMap<&str, Vec<&str>> = HashMap::from_iter(vec![
            ("1-0040", vec!["Sensor 0, channel 0", "Sensor 0, channel 1"]),
            ("1-0041", vec!["Sensor 1, channel 0"]),
        ]);
        let mut expected_metrics = vec!["in_input", "curr_input", "curr_crit", "crit_max"];
        expected_metrics.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<String> = sensor
                .channels
                .iter()
                .map(|chan| chan.label.to_owned().unwrap_or_else(|| format!("channel_{}", chan.id)))
                .collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[sensor.i2c_id.as_str()];
            assert_eq!(expected_labels, &channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();
                assert_eq!(metrics, expected_metrics);
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

        let sensor_0040_link = root.join("1-0041");
        let sensor_0041_link = root.join("1-0040");
        let sensor_0040 = actual_root.join("1-0040");
        let sensor_0041 = actual_root.join("1-0041");
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
        let sensors = detect_hierarchy_modern(root).expect("detection failed");
        println!("detected: {sensors:?}");

        let mut sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        sensor_ids.sort();
        assert_eq!(sensor_ids, vec!["1-0040", "1-0041"]);

        let expected_channel_labels: HashMap<&str, Vec<&str>> = HashMap::from_iter(vec![
            ("1-0040", vec!["Sensor 0, channel 0", "Sensor 0, channel 1"]),
            ("1-0041", vec!["Sensor 1, channel 0"]),
        ]);
        let mut expected_metrics = vec!["in_input", "curr_input", "curr_crit", "crit_max"];
        expected_metrics.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<String> = sensor
                .channels
                .iter()
                .map(|chan| chan.label.to_owned().unwrap_or_else(|| format!("channel_{}", chan.id)))
                .collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[sensor.i2c_id.as_str()];
            assert_eq!(expected_labels, &channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();
                assert_eq!(metrics, expected_metrics);
            }
        }
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
        let sensors = detect_hierarchy_old_v4(root).expect("detection failed");
        let mut sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        sensor_ids.sort();
        assert_eq!(sensor_ids, vec!["1-0040", "1-0041"]);

        let expected_channel_labels: HashMap<&str, Vec<&str>> = HashMap::from_iter(vec![
            ("1-0040", vec!["Sensor 0, channel 0", "Sensor 0, channel 1"]),
            ("1-0041", vec!["Sensor 1, channel 0"]),
        ]);
        let mut expected_metrics = vec![
            "in_current_input",
            "in_voltage_input",
            "in_power_input",
            "crit_current_limit",
            "warn_current_limit",
        ];
        expected_metrics.sort();

        for sensor in sensors.into_iter() {
            let mut channel_labels: Vec<String> = sensor
                .channels
                .iter()
                .map(|chan| chan.label.to_owned().unwrap_or_else(|| format!("channel_{}", chan.id)))
                .collect();
            channel_labels.sort();

            let expected_labels = &expected_channel_labels[sensor.i2c_id.as_str()];
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
        let res_modern = detect_hierarchy_modern(&root);
        let res_old = detect_hierarchy_old_v4(&root);
        assert!(matches!(res_modern, Err(DetectionError::SysfsAccess(_))));
        assert!(matches!(res_old, Err(DetectionError::SysfsAccess(_))));
    }
}
