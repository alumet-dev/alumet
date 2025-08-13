//! System-level metrics.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek},
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;

/// Reads memory status from /proc/meminfo.
pub struct MeminfoProbe {
    /// A reader opened to /proc/meminfo.
    reader: BufReader<File>,
    /// List of metrics to read, other metrics are ignored when reading the file.
    /// Every metric should have the `Byte` unit.
    metrics_to_read: Vec<(String, TypedMetricId<u64>)>,
}

impl MeminfoProbe {
    pub fn new(
        mut metrics_to_read: Vec<(String, TypedMetricId<u64>)>,
        proc_meminfo_path: &str,
    ) -> anyhow::Result<Self> {
        // sort the metric by their unique names to speed up search in poll()
        metrics_to_read.sort_unstable_by_key(|(name, _id)| name.to_owned());
        let file = File::open(proc_meminfo_path).with_context(|| format!("could not open {proc_meminfo_path}"))?;
        Ok(Self {
            reader: BufReader::new(file),
            metrics_to_read,
        })
    }
}

impl Source for MeminfoProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        fn parse_meminfo_line(line: &str) -> Option<(&str, u64)> {
            let mut s = line.split_ascii_whitespace();
            let key = s.next()?;
            let value = s.next()?;
            let unit = s.next(); // no unit means that the unit is Byte

            let key = &key[..key.len() - 1]; // there is colon after the metric name
            let value: u64 = value.parse().ok()?;
            let value = match unit {
                Some(unit) => convert_meminfo_to_bytes(value, unit)?,
                None => value,
            };
            Some((key, value))
        }

        // Read memory statistics.
        self.reader.rewind()?;
        for line in (&mut self.reader).lines() {
            // Optimization: we don't use procfs::Meminfo::from_buf_read to avoid a allocating HashMap that we don't need.
            let line = line.context("could not read line from /proc/meminfo")?;
            if !line.is_empty() {
                let (key, value) =
                    parse_meminfo_line(&line).with_context(|| format!("invalid line in /proc/meminfo: {line}"))?;

                if let Ok(i) = self.metrics_to_read.binary_search_by_key(&key, |(name, _)| name) {
                    let metric = self.metrics_to_read[i].1;
                    measurements.push(MeasurementPoint::new(
                        timestamp,
                        metric,
                        Resource::LocalMachine,
                        ResourceConsumer::LocalMachine,
                        value,
                    ));
                }
            }
        }
        Ok(())
    }
}

fn convert_meminfo_to_bytes(value: u64, unit: &str) -> Option<u64> {
    // For meminfo, "kB" actually means "kiB". See the doc of procfs.
    match unit {
        "B" => Some(value),
        "kB" | "KiB" | "kiB" | "KB" => Some(value * 1024),
        "mB" | "MiB" | "miB" | "MB" => Some(value * 1024 * 1024),
        "gB" | "GiB" | "giB" | "GB" => Some(value * 1024 * 1024 * 1024),
        _ => None,
    }
}
