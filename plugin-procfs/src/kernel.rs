//! System-level metrics.

use std::{
    fs::File,
    io::{BufReader, Seek},
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
    pipeline::{elements::error::PollError, Source},
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Context;
use procfs::{
    CpuTime, ExplicitSystemInfo, FromBufReadSI, KernelStats, LocalSystemInfo, ProcError, SystemInfoInterface,
};

/// Reads kernel statistics from /proc/stat.
pub struct KernelStatsProbe {
    /// A reader opened to /proc/stat.
    reader: BufReader<File>,
    sysinfo: ExplicitSystemInfo,

    /// The previously measured stats, to compute the difference.
    previous_stats: Option<KernelStats>,

    // ids of metrics
    metrics: KernelMetrics,
}

pub struct KernelMetrics {
    cpu_time: TypedMetricId<u64>,
    context_switches: TypedMetricId<u64>,
    new_forks: TypedMetricId<u64>,
    n_procs_running: TypedMetricId<u64>,
    n_procs_blocked: TypedMetricId<u64>,
}

fn gather_system_info() -> Result<ExplicitSystemInfo, ProcError> {
    let sysinfo = LocalSystemInfo;
    Ok(ExplicitSystemInfo {
        boot_time_secs: sysinfo.boot_time_secs()?,
        ticks_per_second: sysinfo.ticks_per_second(),
        page_size: sysinfo.page_size(),
        is_little_endian: sysinfo.is_little_endian(),
    })
}

impl KernelMetrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            cpu_time: alumet.create_metric("kernel_cpu_time", PrefixedUnit::milli(Unit::Second), "busy CPU time")?,
            context_switches: alumet.create_metric(
                "kernel_context_switches",
                Unit::Unity,
                "number of context switches",
            )?,
            new_forks: alumet.create_metric("kernel_new_forks", Unit::Unity, "number of fork operations")?,
            n_procs_running: alumet.create_metric(
                "kernel_n_procs_running",
                Unit::Unity,
                "number of processes in a runnable state",
            )?,
            n_procs_blocked: alumet.create_metric(
                "kernel_n_procs_blocked",
                Unit::Unity,
                "numbers of processes that are blocked on I/O operations",
            )?,
        })
    }
}

impl KernelStatsProbe {
    pub fn new(metrics: KernelMetrics, proc_stat_path: &str) -> anyhow::Result<Self> {
        let file = File::open(proc_stat_path).with_context(|| format!("could not open {proc_stat_path}"))?;
        Ok(Self {
            reader: BufReader::new(file),
            sysinfo: gather_system_info().context("could not gather system info")?,
            previous_stats: None,
            metrics,
        })
    }
}

impl Source for KernelStatsProbe {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        self.reader.rewind()?;
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
            cpu_time_total.push_measurements(self.metrics.cpu_time, Resource::LocalMachine, acc, timestamp);
            for (i, cpu_time) in cpu_time_per_cpu.into_iter().enumerate() {
                cpu_time.push_measurements(
                    self.metrics.cpu_time,
                    Resource::CpuCore { id: i as u32 },
                    acc,
                    timestamp,
                )
            }
            acc.push(MeasurementPoint::new(
                timestamp,
                self.metrics.context_switches,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                context_switches,
            ));
            acc.push(MeasurementPoint::new(
                timestamp,
                self.metrics.new_forks,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                new_forks,
            ));
            if let Some(n) = n_procs_running {
                acc.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.n_procs_running,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    n as u64,
                ));
            }
            if let Some(n) = n_procs_blocked {
                acc.push(MeasurementPoint::new(
                    timestamp,
                    self.metrics.n_procs_blocked,
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
