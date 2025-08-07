use std::path::{Path, PathBuf};

use alumet::units::{PrefixedUnit, Unit};
use regex::Regex;
use walkdir::WalkDir;

use super::common::{ChannelEntryAnalyzer, MetricMatcher, METRIC_CURRENT, METRIC_POWER, METRIC_VOLTAGE};
use super::{EntryAnalysis, InaDeviceMetadata, InaExplorer};

pub const SYSFS_INA_OLD: &str = "/sys/bus/i2c/drivers/ina3221x";

/// Detects the available INA sensors, assuming that Nvidia Jetpack version 4.x is installed.
/// The hierarchy is subtly different in that case, and the metric and label files have different names than in v5+.
pub struct OldInaExplorer {
    sysfs_root: PathBuf,
    entry_analyzer: ChannelEntryAnalyzer,
}

impl OldInaExplorer {
    pub fn new(sysfs_root: impl Into<PathBuf>) -> Self {
        Self {
            sysfs_root: sysfs_root.into(),
            entry_analyzer: Self::init_analyzer().expect("regexps should be valid"),
        }
    }

    fn init_analyzer() -> Result<ChannelEntryAnalyzer, regex::Error> {
        Ok(ChannelEntryAnalyzer {
            label_matcher: Regex::new("rail_name(_)?(?<N>[0-9]+)")?,
            metrics_matchers: vec![
                MetricMatcher {
                    pat: Regex::new("in_current(?<N>[0-9]+)_input")?,
                    unit: PrefixedUnit::milli(Unit::Ampere),
                    metric_name: METRIC_CURRENT,
                },
                MetricMatcher {
                    pat: Regex::new("in_voltage(?<N>[0-9]+)_input")?,
                    unit: PrefixedUnit::milli(Unit::Volt),
                    metric_name: METRIC_VOLTAGE,
                },
                MetricMatcher {
                    pat: Regex::new("in_power(?<N>[0-9]+)_input")?,
                    unit: PrefixedUnit::milli(Unit::Watt),
                    metric_name: METRIC_POWER,
                },
            ],
        })
    }
}

impl InaExplorer for OldInaExplorer {
    fn sysfs_root(&self) -> &Path {
        self.sysfs_root.as_path()
    }

    fn devices(&self) -> anyhow::Result<Vec<InaDeviceMetadata>> {
        let mut res = Vec::new();

        for entry in WalkDir::new(self.sysfs_root())
            .min_depth(2)
            .max_depth(2)
            .follow_links(true)
        {
            let entry = match entry {
                Ok(e) => e,
                Err(err) => {
                    if err.io_error().is_some() {
                        return Err(err.into());
                    } else {
                        // a loop has been detected, just ignore it
                        continue;
                    }
                }
            };
            if let Some(device) = parse_device_entry(entry.path()) {
                res.push(device);
            }
        }
        Ok(res)
    }

    fn analyze_entry(&self, channel_entry_path: &Path) -> anyhow::Result<EntryAnalysis> {
        self.entry_analyzer.analyze_entry(channel_entry_path)
    }
}

/// Parses `/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device1`.
/// Extracts `40` (hexadecimal i2c address) and `1` (device id).
fn parse_device_entry(entry_path: &Path) -> Option<InaDeviceMetadata> {
    // get i2c address
    let i2c_identifier = entry_path.parent()?.file_name()?.to_str()?;
    let (_, addr) = i2c_identifier.split_once('-')?;
    let i2c_address = u32::from_str_radix(addr, 16).ok()?;

    // get device id
    let device_identifier = entry_path.file_name()?.to_str()?;
    let number = device_identifier.strip_prefix("iio:device")?.parse().ok()?;

    Some(InaDeviceMetadata {
        path: entry_path.to_owned(),
        i2c_address,
        number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_device_entry() {
        assert_eq!(
            parse_device_entry(Path::new("/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device1")),
            Some(InaDeviceMetadata {
                path: PathBuf::from("/sys/bus/i2c/drivers/ina3221x/7-0040/iio:device1"),
                i2c_address: 64,
                number: 1
            })
        );

        assert_eq!(parse_device_entry(Path::new("/sys/bus/i2c/drivers/ina3221x/bad")), None)
    }
}
