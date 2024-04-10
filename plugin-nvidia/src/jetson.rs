use std::{
    collections::HashMap, fs::File, io::ErrorKind, path::{Path, PathBuf}
};

use alumet::{metrics::TypedMetricId, units::{ScaledUnit, Unit}};
use anyhow::{anyhow, Context};
use regex::Regex;

pub struct InaSensor {
    path: PathBuf,
    i2c_id: String,
    channels: Vec<InaChannel>,
}

pub struct InaChannel {
    label: String,
    metrics: Vec<InaRailMetric>,
}

pub struct InaRailMetric {
    path: PathBuf,
    unit: ScaledUnit,
    name: String,
    // Added in a second pass based on the Jetson documentation.
    description: Option<String>,
}

// pub struct OpenedInaMetric {
//     file: File,
//     id: TypedMetricId<u64>,
// }

pub fn detect_ina_sensors() -> anyhow::Result<Vec<InaSensor>> {
    let mut res = detect_hierarchy_modern(SYSFS_INA)?;
    if res.is_empty() {
        res = detect_hierarchy_old_v4(SYSFS_INA_OLD)?;
    }
    Ok(res)
}

const SYSFS_INA_OLD: &str = "/sys/bus/i2c/drivers/ina3221x";
const SYSFS_INA: &str = "/sys/bus/i2c/drivers/ina3221";

fn detect_hierarchy_modern<P: AsRef<Path>>(sys_ina: P) -> anyhow::Result<Vec<InaSensor>> {
    /// Look for a path of the form <sensor_path>/hwmon/hwmon<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        let hwmon = sensor_path.join("hwmon");
        for child in std::fs::read_dir(&hwmon)
            .with_context(|| format!("failed to list content of directory {}", hwmon.display()))?
        {
            let child = child?;
            let path = child.path();
            if path.file_name().unwrap().to_string_lossy().starts_with("hwmon") {
                return Ok(path);
            }
        }
        Err(anyhow!("not found"))
    }

    /// Look for channels and metrics.
    ///
    /// ## Arguments
    /// - `channels_dir`: path of the form <sensor_path>/hwmon/hwmon<id>
    fn sensor_channels(channels_dir: &Path) -> anyhow::Result<Vec<InaChannel>> {
        fn guess_channel_unit(prefix: &str, _suffix: &str) -> Option<ScaledUnit> {
            match prefix {
                "curr" => Some(ScaledUnit::milli(Unit::Ampere)),
                "in" => Some(ScaledUnit::milli(Unit::Volt)),
                "crit" => Some(ScaledUnit::milli(Unit::Ampere)),
                _ if prefix.contains("volt") => Some(ScaledUnit::milli(Unit::Volt)),
                _ if prefix.contains("current") => Some(ScaledUnit::milli(Unit::Ampere)),
                _ => None,
            }
        }

        let metric_filename_pattern = Regex::new(r"([a-zA-Z]+)(\d+)_([a-zA-Z]+)")?;
        let mut channel_metrics = HashMap::with_capacity(2);
        let mut channel_labels = HashMap::with_capacity(2);
        for entry in std::fs::read_dir(channels_dir)? {
            let entry = entry?;
            let path = entry.path();
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            if let Some(groups) = metric_filename_pattern.captures(&filename) {
                let (prefix, suffix) = (&groups[1], &groups[3]);
                let channel_id: u32 = groups[2]
                    .parse()
                    .with_context(|| format!("Invalid channel id: {}", &groups[2]))?;
                if prefix == "in" && suffix == "label" {
                    // This file contains the label of the channel.
                    let label = std::fs::read_to_string(path)?;
                    channel_labels.insert(channel_id, label);
                } else {
                    // This file contains the (automatically updated) value of a metric.
                    let unit = guess_channel_unit(prefix, suffix).with_context(|| {
                        format!(
                            "Could not guess the unit of this unknown INA3221 metric: {}",
                            path.display()
                        )
                    })?;
                    channel_metrics
                        .entry(channel_id)
                        .or_insert_with(|| Vec::with_capacity(4))
                        .push(InaRailMetric {
                            path,
                            unit,
                            name: format!("{prefix}_{suffix}"),
                            description: None,
                        })
                }
            }
        }
        let res = channel_metrics
            .into_iter()
            .map(|(id, metrics)| InaChannel {
                label: channel_labels.get(&id).map_or_else(|| "?", |v| v).to_owned(),
                metrics,
            })
            .collect();
        Ok(res)
    }

    let dir_path: &Path = sys_ina.as_ref();
    let mut sensors = Vec::new();
    match std::fs::read_dir(dir_path) {
        Ok(dir) => {
            for entry in dir {
                let entry = entry?;
                if entry.metadata()?.is_dir() {
                    // Each subdirectory corresponds to one INA 3221 sensor.
                    let path = entry.path();
                    // The name of the directory corresponds to the i2c id.
                    let i2c_id = path.file_name().unwrap().to_str().unwrap().to_owned();
                    // Discover all the sensor channels (with their metrics).
                    let channels = sensor_channels(&sensor_channels_dir(&path)?)?;
                    sensors.push(InaSensor { path, channels, i2c_id });
                }
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // The directory does not exist, simply return an empty list of sensors.
            ()
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

fn detect_hierarchy_old_v4<P: AsRef<Path>>(old_sys_ina: P) -> anyhow::Result<Vec<InaSensor>> {
    fn guess_channel_unit(prefix: &str, _suffix: &Option<regex::Match>) -> Option<ScaledUnit> {
        if prefix.contains("current") {
            Some(ScaledUnit::milli(Unit::Ampere))
        } else if prefix.contains("voltage") {
            Some(ScaledUnit::milli(Unit::Volt))
        } else if prefix.contains("power") {
            Some(ScaledUnit::milli(Unit::Watt))
        } else {
            None
        }
    }

    /// Look for a path of the form <sensor_path>/iio:device<id>
    fn sensor_channels_dir(sensor_path: &Path) -> anyhow::Result<PathBuf> {
        for child in std::fs::read_dir(sensor_path)? {
            let child = child?;
            let path = child.path();
            if path.file_name().unwrap().to_string_lossy().starts_with("iio:device") {
                return Ok(path);
            }
        }
        Err(anyhow!("not found"))
    }

    /// Look for channels and metrics.
    ///
    /// ## Arguments
    /// - `channels_dir`: path of the form <sensor_path>/iio:device<id>
    fn sensor_channels(channels_dir: &Path) -> anyhow::Result<Vec<InaChannel>> {
        let metric_filename_pattern = Regex::new(r"(?<prefix>[a-zA-Z_]+?)_?(?<id>\d+)(?<suffix>_([a-zA-Z]+))?")?;
        let mut channel_metrics = HashMap::with_capacity(2);
        let mut channel_labels = HashMap::with_capacity(2);
        for entry in std::fs::read_dir(channels_dir)? {
            let entry = entry?;
            let path = entry.path();
            let filename = path.file_name().unwrap().to_string_lossy();
            if let Some(groups) = metric_filename_pattern.captures(&filename) {
                let (prefix, suffix) = (&groups["prefix"], groups.name("suffix"));
                let channel_id: u32 = groups["id"]
                    .parse()
                    .with_context(|| format!("Invalid channel id: {}", &groups["id"]))?;
                if prefix == "rail_name" && suffix == None {
                    // This file contains the label of the channel.
                    let label = std::fs::read_to_string(path)?;
                    channel_labels.insert(channel_id, label);
                } else {
                    // This file contains the (automatically updated) value of a metric.
                    let unit = guess_channel_unit(prefix, &suffix).with_context(|| {
                        format!(
                            "Could not guess the unit of this unknown INA3221 metric: {}",
                            path.display()
                        )
                    })?;
                    let name = match suffix {
                        Some(suffix_match) => format!("{prefix}{}", suffix_match.as_str()),
                        None => prefix.to_string(),
                    };
                    channel_metrics
                        .entry(channel_id)
                        .or_insert_with(|| Vec::with_capacity(5))
                        .push(InaRailMetric {
                            path,
                            unit,
                            name,
                            description: None,
                        })
                }
            }
        }
        let res = channel_metrics
            .into_iter()
            .map(|(id, metrics)| InaChannel {
                label: channel_labels.get(&id).map_or_else(|| "?", |v| v).to_owned(),
                metrics,
            })
            .collect();
        Ok(res)
    }

    let mut sensors = Vec::new();
    for entry in std::fs::read_dir(old_sys_ina)? {
        let entry = entry?;
        if entry.metadata()?.is_dir() {
            // Each subdirectory corresponds to one INA 3221 sensor.
            let path = entry.path();
            // The name of the directory corresponds to the i2c id.
            let i2c_id = path.file_name().unwrap().to_str().unwrap().to_owned();
            // Discover all the sensor channels (with their metrics).
            let channels = sensor_channels(&sensor_channels_dir(&path)?)?;
            sensors.push(InaSensor { path, channels, i2c_id });
        }
    }
    Ok(sensors)
}

// - `/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device0`
// - `/sys/bus/i2c/drivers/ina3221x/1-0041/iio:device1`
// - `/sys/bus/i2c/drivers/ina3221/1-0040/hwmon/hwmon<x>`
// - `/sys/bus/i2c/drivers/ina3221/1-0041/hwmon/hwmon<y>
#[cfg(test)]
mod tests {
    use crate::jetson::detect_hierarchy_old_v4;

    use super::detect_hierarchy_modern;

    #[test]
    fn ina_modern() {
        let tmp = std::env::temp_dir();

        // Create the fake sensor directories
        let root = tmp.join("test-alumet-plugin-nvidia/ina-modern");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
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
        let sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        assert_eq!(sensor_ids, vec!["1-0040", "1-0041"]);

        let expected_channel_labels = vec![
            vec!["Sensor 0, channel 0", "Sensor 0, channel 1"],
            vec!["Sensor 1, channel 0"],
        ];
        let mut expected_metrics = vec!["in_input", "curr_input", "curr_crit", "crit_max"];
        expected_metrics.sort();

        for (sensor, expected_labels) in sensors.into_iter().zip(expected_channel_labels) {
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

    #[test]
    fn ina_old() {
        let tmp = std::env::temp_dir();

        // Create the fake sensor directories
        let root = tmp.join("test-alumet-plugin-nvidia/ina-old");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
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
        let sensor_ids: Vec<&str> = sensors.iter().map(|s| s.i2c_id.as_ref()).collect();
        assert_eq!(sensor_ids, vec!["1-0040", "1-0041"]);

        let expected_channel_labels = vec![
            vec!["Sensor 0, channel 0", "Sensor 0, channel 1"],
            vec!["Sensor 1, channel 0"],
        ];
        let mut expected_metrics = vec![
            "in_current_input",
            "in_voltage_input",
            "in_power_input",
            "crit_current_limit",
            "warn_current_limit",
        ];
        expected_metrics.sort();

        for (sensor, expected_labels) in sensors.into_iter().zip(expected_channel_labels) {
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
}
