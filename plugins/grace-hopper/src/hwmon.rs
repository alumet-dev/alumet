//! Discovery and gathering of Grace Hopper hwmon "devices".

use std::{
    fmt::{Debug, Display},
    fs::File,
    io::{Read, Seek},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, anyhow};
use thiserror::Error;

#[cfg(not(target_os = "linux"))]
compile_error!("only Linux is supported");

/// Represents a hwmon device that exposes GraceHopper power telemetry.
///
/// ## Expected file layout on node {x}
///
/// ```txt
/// /sys/class/hwmon/hwmon{x}/device/
/// |
/// |−− power1_oem_info
/// |−− power1_average
/// |−− power1_average_interval
/// ```
#[derive(Debug)]
pub struct Device {
    pub path: PathBuf,
    pub info: SensorInfo,
    pub power_file: File,
}

impl Device {
    pub fn at_sysfs(path: &Path) -> anyhow::Result<Self> {
        let info_file = path.join("power1_oem_info");
        let power_file = path.join("power1_average");
        // let interval_file = path.join("power1_average_interval"); // we don't use it
        let info = std::fs::read_to_string(&info_file).with_context(|| format!("failed to read {info_file:?}"))?;
        let info = SensorInfo::from_str(&info)
            .with_context(|| format!("failed to parse {info_file:?}: invalid content '{info}'"))?;
        let power_file = File::open(&power_file).with_context(|| format!("failed to open {power_file:?}"))?;
        Ok(Self {
            path: path.to_path_buf(),
            info,
            power_file,
        })
    }

    /// Reads the power consumption of this device.
    ///
    /// According to nvidia's docs, the value that is returned is the **average power
    /// over the past x milliseconds**, where x is given by the content of the file `power1_average_interval`.
    /// The default interval is 50 milliseconds.
    ///
    /// The returned value is in **microWatts**.
    pub fn read_power_value(&mut self, buf: &mut String) -> anyhow::Result<u64> {
        let value = self
            .impl_read_power_value(buf)
            .with_context(|| format!("failed to read power for device {:?}", self.path))?;
        Ok(value)
    }

    fn impl_read_power_value(&mut self, buf: &mut String) -> anyhow::Result<u64> {
        buf.clear();
        self.power_file.rewind()?;
        self.power_file.read_to_string(buf)?;
        let value = buf
            .trim_ascii_end()
            .parse()
            .with_context(|| format!("invalid input {buf:?}"))?;
        Ok(value)
    }
}

/// Explore a tree of hwmon devices.
///
/// ## Expected file layout
///
/// Example of `hwmon_path`: `/sys/fs/hwmon`
/// Example of layout:
/// ```txt
/// /sys/class/hwmon/
/// |
/// |− hwmon1
///     |− device
///         |− power1_oem_info
///         |− power1_average
///         |− …
/// |− hwmon2
/// ```
pub fn explore(hwmon_path: &Path) -> anyhow::Result<Vec<Device>> {
    let mut devices = Vec::with_capacity(6); // we expect 4 or 6 items
    for entry in std::fs::read_dir(hwmon_path).with_context(|| format!("failed to read dir {hwmon_path:?}"))? {
        let entry = entry?;
        let path = entry.path();
        for entry in std::fs::read_dir(&path).with_context(|| format!("failed to read dir {hwmon_path:?}"))? {
            let entry = entry?;
            let path = entry.path();
            let file_type = std::fs::metadata(&path)?.file_type(); // traverses symlinks
            let file_name = path.file_name().unwrap().to_string_lossy();
            if file_name == "device" && file_type.is_dir() {
                log::trace!("inspecting {path:?}");
                // entry is /sys/class/hwmon/hwmonX/device and has a file power1_oem_info
                if std::fs::exists(path.join("power1_oem_info"))? {
                    match Device::at_sysfs(&path) {
                        Ok(device) => devices.push(device),
                        Err(err) => log::error!(
                            "dir {path:?} looks like a Grace/GraceHopper telemetry sensor but we failed to analyze it: {err:?}"
                        ),
                    };
                }
            }
        }
    }

    if devices.is_empty() {
        return Err(anyhow!("power telemetry not found in {hwmon_path:?}"));
    }

    Ok(devices)
}

/// Kind of information provided by the hwmon file.
/// See https://docs.nvidia.com/grace-perf-tuning-guide/power-thermals.html#power-telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, enum_map::Enum)]
pub enum TelemetryKind {
    /// Total power of the socket.
    Grace,
    /// CPU rail power for the socket.
    Cpu,
    /// SOC rail power.
    SysIo,
    /// Total power of the GraceHopper, including regulator loss and DRAM, GPU and HBM power.
    Module,
}

/// Sensor tag, which corresponds to a `TelemetryKind` or to a value computed by the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, enum_map::Enum)]
pub enum SensorTagKind {
    Grace,
    Cpu,
    SysIo,
    Module,
    Dram,
}

impl From<TelemetryKind> for SensorTagKind {
    fn from(value: TelemetryKind) -> Self {
        match value {
            TelemetryKind::Grace => SensorTagKind::Grace,
            TelemetryKind::Cpu => SensorTagKind::Cpu,
            TelemetryKind::SysIo => SensorTagKind::SysIo,
            TelemetryKind::Module => SensorTagKind::Module,
        }
    }
}

#[derive(Debug, Error)]
#[error("invalid telemetry kind")]
pub struct InvalidTelemetryKind;

impl FromStr for TelemetryKind {
    type Err = InvalidTelemetryKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "grace" => Ok(Self::Grace),
            "cpu" => Ok(Self::Cpu),
            "sysio" => Ok(Self::SysIo),
            "module" => Ok(Self::Module),
            _ => Err(InvalidTelemetryKind),
        }
    }
}

impl TelemetryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TelemetryKind::Grace => "grace",
            TelemetryKind::Cpu => "cpu",
            TelemetryKind::SysIo => "sysio",
            TelemetryKind::Module => "module",
        }
    }
}

impl Display for TelemetryKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SensorTagKind {
    pub fn as_str_total(&self) -> &'static str {
        match self {
            Self::Grace => "grace_total",
            Self::Cpu => "cpu_total",
            Self::SysIo => "sysio_total",
            Self::Module => "module_total",
            Self::Dram => "dram_total",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct SensorInfo {
    pub kind: TelemetryKind,
    pub socket: u8,
}

#[derive(Debug, Error)]
#[error("invalid sensor info")]
pub struct InvalidSensorInfo;

impl FromStr for SensorInfo {
    type Err = InvalidSensorInfo;

    /// Extracts info from the string that can be found in the file `power1_oem_info` of the hwmon sysfs.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim_ascii().to_ascii_lowercase();
        let (kind, socket) = s.split_once(" power socket ").ok_or(InvalidSensorInfo)?;
        let kind = kind.parse().map_err(|_| InvalidSensorInfo)?;
        let socket = socket.parse().map_err(|_| InvalidSensorInfo)?;
        Ok(SensorInfo { kind, socket })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn parse_sensor_information() {
        assert_eq!(
            SensorInfo::from_str("Grace Power Socket 0").unwrap(),
            SensorInfo {
                kind: TelemetryKind::Grace,
                socket: 0
            }
        );
        assert_eq!(
            SensorInfo::from_str("CPU Power Socket 0").unwrap(),
            SensorInfo {
                kind: TelemetryKind::Cpu,
                socket: 0
            }
        );
        assert_eq!(
            SensorInfo::from_str("CPU Power Socket 1").unwrap(),
            SensorInfo {
                kind: TelemetryKind::Cpu,
                socket: 1
            }
        );
        assert_eq!(
            SensorInfo::from_str("SysIO Power Socket 1").unwrap(),
            SensorInfo {
                kind: TelemetryKind::SysIo,
                socket: 1
            }
        );
        assert_eq!(
            SensorInfo::from_str("Module Power Socket 0").unwrap(),
            SensorInfo {
                kind: TelemetryKind::Module,
                socket: 0
            }
        );
    }

    #[test]
    fn explore_error_not_dir() {
        let root = tempdir().unwrap();
        let file_path = root.path().join("Clara Oswald");
        let _ = File::create(&file_path).unwrap();
        explore(&file_path).expect_err("should fail because this is not a dir");
    }

    #[test]
    fn explore_should_find_devices() -> anyhow::Result<()> {
        let root = tempdir()?;
        let root_path = root.path();
        // device 1
        let file_path_info = root_path.join("mySensor/device/power1_oem_info");
        let file_path_power = root_path.join("mySensor/device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "Module Power Socket 0")?;
        std::fs::write(file_path_power, "123456789")?;

        // device 2
        let file_path_info = root_path.join("hwmon2/device/power1_oem_info");
        let file_path_power = root_path.join("hwmon2/device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "Grace Power Socket 7")?;
        std::fs::write(file_path_power, "5")?;

        // not a grace telemetry device (should not be included in the list of devices)
        let file_path_info = root_path.join("other/something");
        let file_path_power = root_path.join("other/something_else");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "humhum")?;
        std::fs::write(file_path_power, "no")?;

        let mut devices = explore(root_path)?;
        devices.sort_by_key(|d| d.info.socket);
        assert_eq!(devices.len(), 2);
        assert_eq!(
            devices[0].info,
            SensorInfo {
                kind: TelemetryKind::Module,
                socket: 0
            }
        );
        assert_eq!(
            devices[1].info,
            SensorInfo {
                kind: TelemetryKind::Grace,
                socket: 7
            }
        );
        let mut buf = String::new();
        assert_eq!(devices[0].read_power_value(&mut buf)?, 123456789);
        assert_eq!(devices[1].read_power_value(&mut buf)?, 5);
        Ok(())
    }

    #[test]
    fn explore_should_find_devices_with_symlinks() -> anyhow::Result<()> {
        let root = tempdir()?;
        let actual_root = root.path().join("actual");
        let root_path = root.path().join("links");
        std::fs::create_dir(&actual_root)?;
        std::fs::create_dir(&root_path)?;

        // device hwmon0
        let device_link = root_path.join("hwmon0");
        let device_actual = actual_root.join("mySensor");
        std::fs::create_dir(&device_actual)?;
        std::os::unix::fs::symlink(&device_actual, &device_link)?;

        let file_path_info = device_actual.join("device/power1_oem_info");
        let file_path_power = device_actual.join("device/power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "Module Power Socket 0")?;
        std::fs::write(file_path_power, "123456789")?;

        // device hwmon2
        let device_link = root_path.join("hwmon2");
        let device_actual = actual_root.join("hwmon2");
        std::fs::create_dir(&device_actual)?;
        std::os::unix::fs::symlink(&device_actual, &device_link)?;

        // NB: hwmon2/device is a link too!
        let device_device_actual = device_actual.join("device_actual");
        let device_device_link = device_actual.join("device");
        std::fs::create_dir(&device_device_actual)?;
        std::os::unix::fs::symlink(&device_device_actual, &device_device_link)?;

        let file_path_info = device_device_link.join("power1_oem_info");
        let file_path_power = device_device_link.join("power1_average");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "Grace Power Socket 7")?;
        std::fs::write(file_path_power, "5")?;

        // not a grace telemetry device (should not be included in the list of devices)
        let device_link = root_path.join("other");
        let device_actual = actual_root.join("badbad");
        std::fs::create_dir(&device_actual)?;
        std::os::unix::fs::symlink(&device_actual, &device_link)?;
        let file_path_info = device_actual.join("device/something");
        let file_path_power = device_actual.join("something_else");
        std::fs::create_dir_all(file_path_info.parent().unwrap())?;
        std::fs::write(file_path_info, "humhum")?;
        std::fs::write(file_path_power, "no")?;

        let mut devices = explore(&root_path)?;
        devices.sort_by_key(|d| d.info.socket);
        assert_eq!(devices.len(), 2);
        assert_eq!(
            devices[0].info,
            SensorInfo {
                kind: TelemetryKind::Module,
                socket: 0
            }
        );
        assert_eq!(
            devices[1].info,
            SensorInfo {
                kind: TelemetryKind::Grace,
                socket: 7
            }
        );
        let mut buf = String::new();
        assert_eq!(devices[0].read_power_value(&mut buf)?, 123456789);
        assert_eq!(devices[1].read_power_value(&mut buf)?, 5);
        Ok(())
    }
}
