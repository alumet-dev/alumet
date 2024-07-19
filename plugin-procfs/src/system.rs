//! System-level metrics.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek},
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use procfs::{CpuTime, Current, ExplicitSystemInfo, FromBufReadSI, KernelStats};

/// Reads kernel statistics from /proc/stat.
pub struct KernelStatsProbe {
    /// A reader opened to /proc/stat.
    reader: BufReader<File>,
    sysinfo: ExplicitSystemInfo,

    /// The previously measured stats, to compute the difference.
    previous_stats: Option<KernelStats>,

    // metrics
    metric_cpu_time: TypedMetricId<u64>,
    metric_context_switches: TypedMetricId<u64>,
    metric_new_forks: TypedMetricId<u64>,
    metric_n_procs_running: TypedMetricId<u64>,
    metric_n_procs_blocked: TypedMetricId<u64>,
}

impl Source for KernelStatsProbe {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let now = KernelStats::from_buf_read(&mut self.reader, &self.sysinfo)?;
        if let Some(prev) = self.previous_stats.take() {
            // Gather metrics
            let cpu_time_total = DeltaCpuTime::compute_diff(&prev.total, &now.total);
            let cpu_time_per_cpu: Vec<DeltaCpuTime> = prev
                .cpu_time
                .iter()
                .zip(&now.cpu_time)
                .map(|(prev, now)| DeltaCpuTime::compute_diff(prev, now))
                .collect();
            let context_switches = now.ctxt - prev.ctxt;
            let new_forks = now.processes - prev.processes;
            let n_procs_running = now.procs_running;
            let n_procs_blocked = now.procs_blocked;

            // Push measurement points
            cpu_time_total.push_measurements(self.metric_cpu_time, Resource::LocalMachine, acc, timestamp);
            for (i, cpu_time) in cpu_time_per_cpu.into_iter().enumerate() {
                cpu_time.push_measurements(self.metric_cpu_time, Resource::CpuCore { id: i as u32 }, acc, timestamp)
            }
            acc.push(MeasurementPoint::new(
                timestamp,
                self.metric_context_switches,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                context_switches,
            ));
            acc.push(MeasurementPoint::new(
                timestamp,
                self.metric_new_forks,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                new_forks,
            ));
            if let Some(n) = n_procs_running {
                acc.push(MeasurementPoint::new(
                    timestamp,
                    self.metric_n_procs_running,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    n as u64,
                ));
            }
            if let Some(n) = n_procs_blocked {
                acc.push(MeasurementPoint::new(
                    timestamp,
                    self.metric_n_procs_blocked,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    n as u64,
                ));
            }
        }
        self.previous_stats = Some(now);
        Ok(())
    }
}

struct DeltaCpuTime {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    // iowait skipped because it's documented as unreliable
    pub irq: Option<u64>,
    pub softirq: Option<u64>,
    pub steal: Option<u64>,
    pub guest: Option<u64>,
    pub guest_nice: Option<u64>,
}

impl DeltaCpuTime {
    pub fn compute_diff(prev: &CpuTime, now: &CpuTime) -> Self {
        Self {
            user: now.user_ms() - prev.user_ms(),
            nice: now.nice_ms() - prev.nice_ms(),
            system: now.system_ms() - prev.system_ms(),
            idle: now.idle_ms() - prev.idle_ms(),
            irq: now.irq_ms().map(|x| x - prev.irq_ms().unwrap()),
            softirq: now.softirq_ms().map(|x| x - prev.softirq_ms().unwrap()),
            steal: now.steal_ms().map(|x| x - prev.steal_ms().unwrap()),
            guest: now.guest_ms().map(|x| x - prev.guest_ms().unwrap()),
            guest_nice: now.guest_nice_ms().map(|x| x - prev.guest_nice_ms().unwrap()),
        }
    }

    /// Push measurement points with the delta values to `acc`.
    ///
    /// The `metric` must have the `millisecond` unit.
    pub fn push_measurements(
        &self,
        metric: TypedMetricId<u64>,
        res: Resource,
        acc: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) {
        let consumer = ResourceConsumer::LocalMachine;
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.user)
                .with_attr("cpu_state", "user"),
        );
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.nice)
                .with_attr("cpu_state", "nice"),
        );
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.system)
                .with_attr("cpu_state", "system"),
        );
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.idle)
                .with_attr("cpu_state", "idle"),
        );
        if let Some(irq) = self.irq {
            acc.push(
                MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), irq)
                    .with_attr("cpu_state", "irq"),
            );
        }
        if let Some(softirq) = self.softirq {
            acc.push(
                MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), softirq)
                    .with_attr("cpu_state", "softirq"),
            );
        }
        if let Some(steal) = self.steal {
            acc.push(
                MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), steal)
                    .with_attr("cpu_state", "steal"),
            );
        }
        if let Some(guest) = self.guest {
            acc.push(
                MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), guest)
                    .with_attr("cpu_state", "guest"),
            );
        }
        if let Some(guest_nice) = self.guest_nice {
            acc.push(
                MeasurementPoint::new(timestamp, metric, res, consumer, guest_nice)
                    .with_attr("cpu_state", "guest_nice"),
            );
        }
    }
}

/// Reads memory status from /proc/meminfo.
pub struct MeminfoProbe {
    /// A reader opened to /proc/meminfo.
    reader: BufReader<File>,
    /// List of metrics to read, other metrics are ignored when reading the file.
    /// Every metric should have the `Byte` unit.
    metrics_to_read: Vec<(String, TypedMetricId<u64>)>,
}

impl MeminfoProbe {
    pub fn new(mut metrics_to_read: Vec<(String, TypedMetricId<u64>)>) -> procfs::ProcResult<Self> {
        metrics_to_read.sort_unstable_by_key(|(name, _id)| name.to_owned());
        Ok(Self {
            reader: BufReader::new(File::open(procfs::Meminfo::PATH)?),
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
        self.reader.rewind()?;
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
