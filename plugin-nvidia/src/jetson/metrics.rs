use anyhow::{anyhow, Context};
use regex::{Match, Regex};
use std::{
    collections::HashMap,
    fs::{read_dir, read_to_string},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use alumet::units::{PrefixedUnit, Unit};

use crate::jetson::utils::*;

const SYSFS_INA_OLD: &str = "/sys/bus/i2c/drivers/ina3221x";
const SYSFS_INA: &str = "/sys/bus/i2c/drivers/ina3221";

/// Cut element in path name to associating and identifying its data with the correct unity.
/// Fix units measured with `PrefixedUnit`.
fn guess_channel_unit(prefix: &Option<Match>) -> Option<PrefixedUnit> {
    match prefix {
        Some(pre) => {
            let measure = pre.as_str();
            match measure {
                "curr" => Some(PrefixedUnit::milli(Unit::Ampere)),
                "in" => Some(PrefixedUnit::milli(Unit::Volt)),
                "crit" => Some(PrefixedUnit::milli(Unit::Ampere)),
                _ if measure.contains("current") => Some(PrefixedUnit::milli(Unit::Ampere)),
                _ if measure.contains("power") => Some(PrefixedUnit::milli(Unit::Watt)),
                _ if measure.contains("volt") => Some(PrefixedUnit::milli(Unit::Volt)),
                _ if measure.contains("shunt") => Some(PrefixedUnit::milli(Unit::Custom {
                    unique_name: "Ohm".to_string(),
                    display_name: "Ω".to_string(),
                })),
                _ => None,
            }
        }
        None => None,
    }
}

/// Detection function, common to all Jetpack versions.
fn detect_hierarchy<P: AsRef<Path>>(
    sys_ina: P,
    metric_filename_pattern: Regex,
    sensor_channels_dir: fn(sensor_path: &Path) -> anyhow::Result<PathBuf>,
    guess_channel_unit: fn(prefix: &Option<Match>) -> Option<PrefixedUnit>,
    is_label_file: fn(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool>,
    format_metric_name: fn(prefix: &Option<Match>, suffix: &Option<Match>) -> String,
) -> anyhow::Result<Vec<InaSensor>> {
    // Look for channels and metrics.
    // - `channels_dir`: path of the form <sensor_path>/hwmon/hwmon<id>
    let sensor_channels = |channels_dir: &Path| -> anyhow::Result<Vec<InaChannel>> {
        let mut channel_metrics = HashMap::with_capacity(2);
        let mut channel_labels = HashMap::with_capacity(2);

        for entry in read_dir(channels_dir)? {
            let entry = entry?;
            let path = entry.path();
            let filename = path.file_name().unwrap().to_string_lossy().to_string();

            if let Some(groups) = metric_filename_pattern.captures(&filename) {
                // Extract the prefix, suffix and channel id.
                let (prefix, suffix) = (&groups.name("prefix"), &groups.name("suffix"));
                let channel_id = groups["id"]
                    .parse()
                    .with_context(|| format!("Invalid channel id: {}", &groups["id"]))?;

                // Determine whether the file contains the label of the channel, or a metric.
                let is_label = is_label_file(prefix, suffix)
                    .with_context(|| format!("Failed to parse filename of INA metric: {}", filename))?;

                if is_label {
                    // This file contains the label of the channel.
                    let label = read_to_string(path)?;
                    channel_labels.insert(channel_id, label);
                } else {
                    // This file contains the (automatically updated) value of a metric.
                    let unit = guess_channel_unit(prefix).with_context(|| {
                        format!(
                            "Could not guess the unit of this unknown INA3221 metric: {}",
                            path.display()
                        )
                    })?;
                    channel_metrics
                        .entry(channel_id)
                        .or_insert_with(|| Vec::with_capacity(5))
                        .push(InaRailMetric {
                            path,
                            unit,
                            name: format_metric_name(prefix, suffix),
                        })
                }
            }
        }

        let res = channel_metrics
            .into_iter()
            .map(|(id, metrics)| InaChannel {
                id,
                label: channel_labels.get(&id).map_or_else(|| "?", |v| v).to_owned(),
                metrics,
                description: None, // added later
            })
            .collect();
        Ok(res)
    };

    let dir_path = sys_ina.as_ref();
    let mut sensors = Vec::new();

    match read_dir(dir_path) {
        Ok(dir) => {
            for entry in dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        eprintln!("Error reading directory entry: {}", e);
                        continue;
                    }
                };
                // Detect is path correspond to a directory, or a link directory
                match entry.path().canonicalize() {
                    Ok(canonical_path) => {
                        if canonical_path.is_dir() {
                            // Each subdirectory corresponds to one INA 3221 sensor.
                            let path = entry.path();
                            // The name of the directory corresponds to the i2c id.
                            let i2c_id = path.file_name().unwrap().to_str().unwrap().to_owned();
                            // Discover all the sensor channels (with their metrics).
                            match (sensor_channels_dir)(&path) {
                                Ok(channels_dir) => match sensor_channels(&channels_dir) {
                                    Ok(channels) => {
                                        sensors.push(InaSensor { path, channels, i2c_id });
                                    }
                                    Err(e) => {
                                        eprintln!("Failed to discover sensor channels: {}", e);
                                    }
                                },
                                Err(e) => {
                                    eprintln!("Failed to get sensor channels directory: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to canonicalize path: {}", e);
                    }
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // The directory does not exist, simply return an empty list of sensors.
            return Err(anyhow!("Failed to enter in INA directory : {e}"));
        }
        Err(e) => {
            return Err(anyhow!(
                "Failed to list the content of directory '{}': {e}",
                dir_path.display()
            ))
        }
    };
    Ok(sensors)
}

/// Detect the available INA sensors, assuming that Nvidia Jetpack version >= 5.0 is installed.
///
/// The standard `sys_ina` looks like `/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device0`.
fn detect_hierarchy_modern<P: AsRef<Path>>(sys_ina: P) -> anyhow::Result<Vec<InaSensor>> {
    /// Look for a path of the form <sensor_path>/hwmon/hwmon<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        let hwmon = sensor_path.join("hwmon");
        match read_dir(&hwmon) {
            Ok(dir) => {
                for child in dir {
                    match child {
                        Ok(child) => {
                            let path = child.path();
                            if path.file_name().unwrap().to_string_lossy().starts_with("hwmon") {
                                return Ok(path);
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to reading child directory: {}", e);
                            continue;
                        }
                    }
                }
                Err(anyhow!("No hwmon directory found"))
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Err(anyhow!("No hwmon directory found")),
            Err(e) => Err(anyhow!(
                "Failed to list content of directory {}: {}",
                hwmon.display(),
                e
            )),
        }
    }

    fn is_label_file(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool> {
        let prefix = prefix.context("Parsing failed: missing prefix")?.as_str();
        let suffix = suffix.context("Parsing failed: missing suffix")?.as_str();
        Ok(prefix == "in" && suffix == "label")
    }

    fn format_metric_name(prefix: &Option<Match>, suffix: &Option<Match>) -> String {
        format!("{}_{}", prefix.unwrap().as_str(), suffix.unwrap().as_str())
    }

    detect_hierarchy(
        sys_ina,
        Regex::new(r"(?<prefix>[a-zA-Z]+)(?<id>\d+)_(?<suffix>[a-zA-Z]+)")?,
        sensor_channels_dir,
        guess_channel_unit,
        is_label_file,
        format_metric_name,
    )
}

/// Detect the available INA sensors, assuming that Nvidia Jetpack version 4.x is installed.
/// The hierarchy is subtly different in that case, and the metric and label files have different names than in v5+.
///
/// The standard `sys_ina` looks like `/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device0`
fn detect_hierarchy_old_v4<P: AsRef<Path>>(sys_ina: P) -> anyhow::Result<Vec<InaSensor>> {
    /// Look for a path of the form <sensor_path>/iio:device<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        for child in read_dir(sensor_path)? {
            let child = child?;
            let path = child.path();
            if path.file_name().unwrap().to_string_lossy().starts_with("iio:device") {
                return Ok(path);
            }
        }
        Err(anyhow!("not found"))
    }

    fn is_label_file(prefix: &Option<Match>, suffix: &Option<Match>) -> anyhow::Result<bool> {
        let prefix = prefix.context("Parsing failed: missing prefix")?.as_str();
        Ok(prefix == "rail_name" && suffix.is_none())
    }

    fn format_metric_name(prefix: &Option<Match>, suffix: &Option<Match>) -> String {
        let prefix = prefix.unwrap().as_str();
        match suffix {
            Some(suffix_match) => format!("{prefix}{}", suffix_match.as_str()),
            None => prefix.to_string(),
        }
    }

    detect_hierarchy(
        sys_ina,
        Regex::new(r"(?<prefix>[a-zA-Z_]+?)_?(?<id>\d+)(?<suffix>_([a-zA-Z]+))?")?,
        sensor_channels_dir,
        guess_channel_unit,
        is_label_file,
        format_metric_name,
    )
}

/// Returns a list of all the INA sensors available on the machine.
///
/// This function supports multiple version of the NVIDIA Jetpack SDK.
pub fn detect_ina_sensors() -> anyhow::Result<Vec<InaSensor>> {
    let mut res = detect_hierarchy_modern(SYSFS_INA)?;
    if res.is_empty() {
        res = detect_hierarchy_old_v4(SYSFS_INA_OLD)?;
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::{
        panic::catch_unwind,
        fs::{create_dir_all, remove_dir_all, write}, os::unix::fs::symlink, path::PathBuf
    };

    // Test `detect_hierarchy_modern` function
    #[test]
    fn test_detect_hierarchy_modern() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-modern");

        if root.exists() {
            remove_dir_all(&root).unwrap();
        }

        let hwmon0 = root.join("1-0040/hwmon/hwmon0");
        let hwmon1 = root.join("1-0041/hwmon/hwmon1");
        create_dir_all(&hwmon0).unwrap();
        create_dir_all(&hwmon1).unwrap();

        // Create the files that contains the label and metrics
        write(hwmon0.join("in0_label"), "Sensor 0, channel 0").unwrap();
        write(hwmon0.join("curr0_input"), "0").unwrap();
        write(hwmon0.join("in0_input"), "1").unwrap();
        write(hwmon0.join("curr0_crit"), "2").unwrap();
        write(hwmon0.join("crit0_max"), "3").unwrap();
        write(hwmon0.join("in1_label"), "Sensor 0, channel 1").unwrap();
        write(hwmon0.join("curr1_input"), "10").unwrap();
        write(hwmon0.join("in1_input"), "11").unwrap();
        write(hwmon0.join("curr1_crit"), "12").unwrap();
        write(hwmon0.join("crit1_max"), "13").unwrap();

        write(hwmon1.join("in0_label"), "Sensor 1, channel 0").unwrap();
        write(hwmon1.join("curr0_input"), "100").unwrap();
        write(hwmon1.join("in0_input"), "101").unwrap();
        write(hwmon1.join("curr0_crit"), "102").unwrap();
        write(hwmon1.join("crit0_max"), "103").unwrap();

        let sensors = detect_hierarchy_modern(root).expect("detection failed");
        let sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        assert_eq!(sensor_ids, vec!["1-0041", "1-0040"]);

        let expected_channel_labels = vec![
            vec!["Sensor 1, channel 0"],
            vec!["Sensor 0, channel 0", "Sensor 0, channel 1"],
        ];
        let mut expected_metrics = vec![
            vec!["crit_max", "curr_crit", "curr_input", "in_input"],
            vec!["crit_max", "curr_crit", "curr_input", "in_input"],
        ];
        expected_metrics.sort();

        for (sensor, expected_labels) in sensors.into_iter().zip(expected_channel_labels) {
            let mut channel_labels: Vec<&String> = sensor.channels.iter().map(|chan| &chan.label).collect();
            channel_labels.sort();
            assert_eq!(expected_labels, channel_labels);

            for (channel, expected_metric) in sensor.channels.iter().zip(expected_metrics.iter()) {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();
                assert_eq!(metrics, *expected_metric)
            }
        }
    }

    // Test `detect_hierarchy_old_v4` function
    #[test]
    fn test_detect_hierarchy_old_v4() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-nvidia/ina-old");

        if root.exists() {
            remove_dir_all(&root).unwrap();
        }

        let device0 = root.join("1-0040/iio:device0");
        let device1 = root.join("1-0041/iio:device1");
        create_dir_all(&device0).unwrap();
        create_dir_all(&device1).unwrap();

        // Create the files that contains the label and metrics
        write(device0.join("rail_name_0"), "Sensor 0, channel 0").unwrap();
        write(device0.join("in_current0_input"), "0").unwrap();
        write(device0.join("in_voltage0_input"), "1").unwrap();
        write(device0.join("in_power0_input"), "2").unwrap();
        write(device0.join("crit_current_limit_0"), "3").unwrap();
        write(device0.join("warn_current_limit_0"), "4").unwrap();

        write(device0.join("rail_name_1"), "Sensor 0, channel 1").unwrap();
        write(device0.join("in_current1_input"), "10").unwrap();
        write(device0.join("in_voltage1_input"), "11").unwrap();
        write(device0.join("in_power1_input"), "12").unwrap();
        write(device0.join("crit_current_limit_1"), "13").unwrap();
        write(device0.join("warn_current_limit_1"), "14").unwrap();

        write(device1.join("rail_name_0"), "Sensor 1, channel 0").unwrap();
        write(device1.join("in_current0_input"), "100").unwrap();
        write(device1.join("in_voltage0_input"), "101").unwrap();
        write(device1.join("in_power0_input"), "102").unwrap();
        write(device1.join("crit_current_limit_0"), "103").unwrap();
        write(device1.join("warn_current_limit_0"), "104").unwrap();

        // Test the detection
        let sensors = detect_hierarchy_old_v4(root).expect("detection failed");
        let sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        assert_eq!(sensor_ids, vec!["1-0041", "1-0040"]);

        let expected_channel_labels = vec![
            vec!["Sensor 1, channel 0"],
            vec!["Sensor 0, channel 0", "Sensor 0, channel 1"],
        ];
        let mut expected_metrics = vec![
            "in_current_input",
            "in_voltage_input",
            "in_power_input",
            "crit_current_limit",
            "warn_current_limit",
        ];
        expected_metrics.sort();

        for (sensor, expected_labels) in sensors.into_iter().zip(expected_channel_labels.clone()) {
            let mut channel_labels: Vec<&String> = sensor.channels.iter().map(|chan| &chan.label).collect();
            channel_labels.sort();
            assert_eq!(expected_labels, channel_labels);

            for channel in sensor.channels {
                let mut metrics: Vec<&String> = channel.metrics.iter().map(|m| &m.name).collect();
                metrics.sort();
                assert_eq!(metrics, expected_metrics);
            }
        }
    }

    // Test `detect_hierarchy_modern` and `detect_hierarchy_old_v4` functions without INA file system
    #[test]
    fn test_without_detected_ina_hierarchy() {
        let result = detect_hierarchy_modern(PathBuf::new());
        assert!(result.is_err());
        let result = detect_hierarchy_old_v4(PathBuf::new());
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_hierarchy_dir_error() {
        let result = detect_hierarchy(
            PathBuf::from("/root"),
            Regex::new(r"(?<prefix>[a-zA-Z]+)(?<id>\d+)_(?<suffix>[a-zA-Z]+)").unwrap(),
            |path| Ok(path.to_path_buf()),
            |_prefix| Some(PrefixedUnit::milli(Unit::Ampere)),
            |_prefix, _suffix| Ok(false),
            |_prefix, _suffix| "metric_name".to_string(),
        );
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Failed to list the content of directory"));
        }
    }

    // Test `detect_hierarchy` function with creation of broken symbolic link
    // to force the canonicalisation error while by directory access
    #[test]
    fn test_detect_hierarchy_failed_canonicalize_path() -> anyhow::Result<()> {
        let root = tempdir()?.path().join("ina3221");
        create_dir_all(&root)?;

        symlink("invalid", root.join("broken_link"))?;

        let metric_filename_pattern = Regex::new(r"(?<prefix>[a-zA-Z]+)(?<id>\d+)_(?<suffix>[a-zA-Z]+)")?;
        let sensor_channels_dir = |_sensor_path: &Path| -> anyhow::Result<PathBuf> {
            Ok(_sensor_path.to_path_buf())
        };
        let guess_channel_unit = |_prefix: &Option<Match>| -> Option<PrefixedUnit> { None };
        let is_label_file = |_prefix: &Option<Match>, _suffix: &Option<Match>| -> anyhow::Result<bool> {
            Ok(false)
        };
        let format_metric_name = |_prefix: &Option<Match>, _suffix: &Option<Match>| -> String {
            "mock".to_string()
        };

        let result = catch_unwind(|| {
            let _ = detect_hierarchy(
                &root,
                metric_filename_pattern,
                sensor_channels_dir,
                guess_channel_unit,
                is_label_file,
                format_metric_name,
            );
        });

        assert!(result.is_ok());
        Ok(())
    }

    // Test `detect_hierarchy` function with channel access error simulated
    #[test]
    fn test_detect_hierarchy_failed_sensor_channels_discovery() -> anyhow::Result<()> {
        let sys_ina_path = tempdir()?.path().join("ina3221");
        create_dir_all(&sys_ina_path)?;
        let sensor_path = sys_ina_path.join("1-0040");
        create_dir_all(&sensor_path)?;

        let sensor_channels_dir = |_sensor_path: &Path| -> anyhow::Result<PathBuf> {
            Err(anyhow!("Error occurs during directory reading"))
        };

        let metric_filename_pattern = Regex::new(r"(?<prefix>[a-zA-Z]+)(?<id>\d+)_(?<suffix>[a-zA-Z]+)")?;
        let guess_channel_unit = |_prefix: &Option<Match>| -> Option<PrefixedUnit> { None };
        let is_label_file = |_prefix: &Option<Match>, _suffix: &Option<Match>| -> anyhow::Result<bool> {
            Ok(false)
        };
        let format_metric_name = |_prefix: &Option<Match>, _suffix: &Option<Match>| -> String {
            "mock".to_string()
        };

        let result = catch_unwind(|| {
            let _ = detect_hierarchy(
                &sys_ina_path,
                metric_filename_pattern,
                sensor_channels_dir,
                guess_channel_unit,
                is_label_file,
                format_metric_name,
            );
        });

        assert!(result.is_ok());
        Ok(())
    }

    // Test `detect_ina_sensors` function
    #[test]
    fn test_detect_ina_sensors_with_detect_hierarchy_modern_error() {
        assert!(detect_ina_sensors().is_err());
    }
}
