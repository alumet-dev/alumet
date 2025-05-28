mod probe;

use anyhow::{anyhow, Context};
use probe::GraceHopperProbe;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, DirEntry, File},
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

#[derive(Debug)]
pub struct Sensor {
    sensor: String,
    socket: u32,
    average_interval: Duration,
    file: PathBuf,
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
        let entries = fs::read_dir(base_dir)?;
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let Some(sensor) = get_sensor_from_dir(entry)? else {
                continue;
            };
            let name = format!("{}_{}", sensor.sensor.clone(), sensor.socket.clone());
            let source = Box::new(GraceHopperProbe::new(alumet, sensor)?);
            alumet.add_source(
                name.as_str(),
                source,
                TriggerSpec::at_interval(self.config.poll_interval),
            )?;
        }
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Attempts to parse and return a [Sensor] from a given directory `entry`.
/// It first checks whether the entry is a directory, and then verifies the presence
/// of all required files within it.
///
/// During processing, it performs various parsing operations and notably makes use
/// of the [get_sensor_information_from_file()] function to extract sensor data.
///
/// Returns a [Sensor] structure wrapped
fn get_sensor_from_dir(entry: DirEntry) -> Result<Option<Sensor>, anyhow::Error> {
    let path = entry.path();
    // Check if it's a directory
    if !path.is_dir() {
        return Err(anyhow!("path is not a directory"));
    }
    let device_path = path.join("device");
    let device_file = device_path.join("power1_oem_info");
    let power_stats_interval_file = device_path.join("power1_average_interval");
    let interval = match power_stats_interval_file.exists() {
        true => {
            let content_file = fs::read_to_string(&power_stats_interval_file).unwrap_or("".to_owned());
            match content_file.trim().parse::<u64>() {
                Ok(ms) => Duration::from_millis(ms),
                Err(e) => {
                    log::error!(
                        "cannot parse the duration (in ms) for {:?}, content: {:?}. Error is: {:?}",
                        power_stats_interval_file,
                        content_file,
                        e
                    );
                    Duration::from_millis(50)
                }
            }
        }
        false => Duration::from_millis(50),
    };
    // Check if file "power1_oem_info" exist0
    if !device_file.exists() {
        return Ok(None);
    }
    let file = File::open(&device_file).context("failed to open the file")?;
    let (sensor, socket) = get_sensor_information_from_file(file)?;
    Ok(Some(Sensor {
        sensor,
        socket,
        average_interval: interval,
        file: device_file,
    }))
}

/// Parses sensor information from a given `file`.
/// It reads the entire content of the provided
/// file, then parses the data to extract relevant information such as the type of sensor
/// and the associated socket number.
///
/// Returns a [Result] containing a tuple `(String, u32)` on success
/// - The first element of the tuple (`String`) represents the sensor type (e.g., "grace", "cpu").
/// - The second element (`u32`) represents the associated socket number.
fn get_sensor_information_from_file(file: File) -> Result<(String, u32), anyhow::Error> {
    let reader = io::BufReader::new(&file);
    for line in reader.lines() {
        let line = line.context("failed to read the line from file")?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 {
            let kind = parts[0].to_string();
            let socket = parts[3]
                .parse::<u32>()
                .context(format!("can't parse the socket to u32, content is: {:?}", parts[3]))?;
            return Ok((kind, socket));
        }
    }
    // Return an error if no valid line found
    Err(anyhow::anyhow!("can't parse the content of the file: {:?}", file))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Initial interval between two measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Path to check hwmon.
    pub root_path: String,
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
    use anyhow::Result;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use tempfile::tempdir;

    #[test]
    fn test_parse_sensor_information() {
        let test_cases = vec![
            ("Module Power Socket 2", "Module", 2),
            ("Grace Power Socket 2", "Grace", 2),
            ("CPU Power Socket 2", "CPU", 2),
            ("SysIO Power Socket 2", "SysIO", 2),
            ("Module Power Socket 3", "Module", 3),
            ("Grace Power Socket 3", "Grace", 3),
            ("CPU Power Socket 3", "CPU", 3),
            ("SysIO Power Socket 3", "SysIO", 3),
            ("Module Power Socket 0", "Module", 0),
            ("Grace Power Socket 0", "Grace", 0),
            ("CPU Power Socket 0", "CPU", 0),
            ("SysIO Power Socket 0", "SysIO", 0),
            ("Module Power Socket 1", "Module", 1),
            ("Grace Power Socket 1", "Grace", 1),
            ("CPU Power Socket 1", "CPU", 1),
            ("SysIO Power Socket 1", "SysIO", 1),
        ];

        for (line, expected_sensor, expected_socket) in test_cases {
            let root = tempdir().unwrap();
            let file_path = root.path().join("power1_oem");
            let mut file = File::create(&file_path).unwrap();
            writeln!(file, "{}", line).unwrap();
            let file = File::open(&file_path)
                .context("Failed to open the file")
                .expect("Can't open the file when testing");
            let result = get_sensor_information_from_file(file);
            assert!(result.is_ok(), "Expected Ok for input '{}'", line);
            let sensor_struct = result.unwrap();
            // Check content
            assert_eq!(
                sensor_struct.0, expected_sensor,
                "Incorrect sensor for input '{}'",
                line
            );
            assert_eq!(
                sensor_struct.1, expected_socket,
                "Incorrect socket for input '{}'",
                line
            );
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
}
