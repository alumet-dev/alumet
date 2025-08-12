use std::path::Path;

use alumet::units::PrefixedUnit;
use regex::Regex;

use super::EntryAnalysis;

pub const METRIC_CURRENT: &str = "input_current";
pub const METRIC_VOLTAGE: &str = "input_voltage";
pub const METRIC_POWER: &str = "input_power";

/// Regex-based analyzer of I2C sysfs channel entries.
pub struct ChannelEntryAnalyzer {
    /// Matches the file that contains the label of the channel.
    pub label_matcher: Regex,
    /// One matcher for each metric we are interested in.
    pub metrics_matchers: Vec<MetricMatcher>,
}

/// Matches a file that we want to read to obtain measurements.
pub struct MetricMatcher {
    /// Pattern on the filename.
    pub pat: Regex,
    /// Measurement unit.
    pub unit: PrefixedUnit,
    /// Name of the metric.
    pub metric_name: &'static str,
}

impl ChannelEntryAnalyzer {
    pub fn analyze_entry(&self, channel_entry_path: &Path) -> anyhow::Result<EntryAnalysis> {
        let filename = channel_entry_path.file_name().unwrap().to_str().unwrap();

        if let Some(c) = self.label_matcher.captures(filename) {
            // this file contains the label of the I2C channel
            let channel_id_match = c.name("N").unwrap();
            let channel_id: u32 = channel_id_match.as_str().parse()?;
            let label = std::fs::read_to_string(channel_entry_path)?.trim_ascii().to_owned();
            return Ok(EntryAnalysis::Label { channel_id, label });
        }

        for MetricMatcher { pat, unit, metric_name } in &self.metrics_matchers {
            if let Some(c) = pat.captures(filename) {
                // this file contains a value that we want to measure
                let channel_id_match = c.name("N").unwrap();
                let channel_id: u32 = channel_id_match.as_str().parse()?;

                // test the file, if it doesn't work, don't measure it in the future
                let content = std::fs::read_to_string(channel_entry_path)?;
                return if content.is_empty() {
                    Err(anyhow::Error::msg("empty file"))
                } else {
                    Ok(EntryAnalysis::MeasurementNode {
                        channel_id,
                        unit: unit.clone(),
                        metric_name: metric_name.to_string(),
                    })
                };
            }
        }

        // nothing matches, ignore this file
        Ok(EntryAnalysis::Ignore)
    }
}
