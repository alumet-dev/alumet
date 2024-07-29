//! Process-level (by pid) metrics.

use std::{collections::HashMap, fs::File, io::BufReader};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{control::ScopedControlHandle, elements::error::PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use procfs::{self, process::Process, FromRead, ProcError};

fn a() {
    // TODO DiskStat et IoPressure
    // procfs::process::all_processes()
}

/// Reads process stats from `/proc/<pid>` for some `<pid>`.
struct ProcessStatsProbe {
    /// The process id, as reported by the kernel.
    pid: i32,

    /// Millisecond per tick (computed from the "ticks per second" reported by the kernel).
    ///
    /// Useful to compute meaningful time from tick-based values.
    ms_per_ticks: u64,

    /// A reader opened to `/proc/<pid>/stat`
    reader_stat: BufReader<File>,
    /// A reader opened to `/proc/<pid>/statm`
    reader_statm: BufReader<File>,

    /// The previously measured stats, to compute the difference.
    previous_general_stats: Option<procfs::process::Stat>,

    // metrics
    metric_cpu_time: TypedMetricId<u64>,
    metric_memory: TypedMetricId<u64>,
}

impl ProcessStatsProbe {
    fn new(
        process: Process,
        ms_per_ticks: u64,
        metric_cpu_time: TypedMetricId<u64>,
        metric_memory: TypedMetricId<u64>,
    ) -> Result<Self, procfs::ProcError> {
        Ok(Self {
            pid: process.pid,
            ms_per_ticks,
            reader_stat: BufReader::new(process.open_relative("stat")?),
            reader_statm: BufReader::new(process.open_relative("statm")?),
            previous_general_stats: None,
            metric_cpu_time,
            metric_memory,
        })
    }
}

impl Source for ProcessStatsProbe {
    fn poll(&mut self, buffer: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        let consumer = ResourceConsumer::Process { pid: self.pid as u32 };

        let general = match procfs::process::Stat::from_read(&mut self.reader_stat) {
            Err(ProcError::NotFound(_)) => {
                // the process no longer exists, stop the source
                return Err(PollError::NormalStop);
            }
            other => other,
        }?;
        let memory = match procfs::process::StatM::from_read(&mut self.reader_statm) {
            Err(ProcError::NotFound(_)) => {
                // the process no longer exists, stop the source
                return Err(PollError::NormalStop);
            }
            other => other,
        }?;

        // TODO how to report the state of the process in the timeseries?
        // let state = now.state()?;

        // Compute CPU usage in the last time slice.
        if let Some(prev) = self.previous_general_stats.take() {
            let cpu_diff = DeltaCpuTime::compute_diff(&prev, &general, self.ms_per_ticks);
            cpu_diff.push_measurements(
                self.metric_cpu_time,
                Resource::LocalMachine,
                consumer.clone(),
                buffer,
                t,
            );
        }

        // Compute RAM usage in the last time slice.
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory,
                Resource::LocalMachine,
                consumer.clone(),
                memory.resident,
            )
            .with_attr("memory_kind", "resident"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory,
                Resource::LocalMachine,
                consumer.clone(),
                memory.shared,
            )
            .with_attr("memory_kind", "shared"),
        );
        buffer.push(
            MeasurementPoint::new(t, self.metric_memory, Resource::LocalMachine, consumer, memory.size)
                .with_attr("memory_kind", "vmsize"),
        );

        Ok(())
    }
}

struct DeltaCpuTime {
    user: u64,
    system: u64,
    // children_user: u64,
    // children_system: u64,
    guest: Option<u64>,
    // children_guest: u64,
}

impl DeltaCpuTime {
    pub fn compute_first(now: &procfs::process::Stat, ms_per_ticks: u64) -> Self {
        Self {
            user: now.utime * ms_per_ticks,
            system: now.stime * ms_per_ticks,
            guest: now.guest_time.map(|x| x * ms_per_ticks),
        }
    }

    pub fn compute_diff(prev: &procfs::process::Stat, now: &procfs::process::Stat, ms_per_ticks: u64) -> Self {
        Self {
            user: (now.utime - prev.utime) * ms_per_ticks,
            system: (now.stime - prev.stime) * ms_per_ticks,
            // children_user: (now.cutime - prev.cutime) * ms_per_ticks,
            // children_system: (now.cstime - prev.cstime) * ms_per_ticks,
            guest: now.guest_time.map(|x| (x - prev.guest_time.unwrap()) * ms_per_ticks),
            // children_guest: (now.cguest_time - prev.cguest_time) * ms_per_ticks,
        }
    }

    pub fn push_measurements(
        &self,
        metric: TypedMetricId<u64>,
        res: Resource,
        consumer: ResourceConsumer,
        acc: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) {
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.user)
                .with_attr("cpu_state", "user"),
        );
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.system)
                .with_attr("cpu_state", "system"),
        );
        if let Some(guest) = self.guest {
            acc.push(MeasurementPoint::new(timestamp, metric, res, consumer, guest).with_attr("cpu_state", "guest"));
        }
    }
}

/// Detects new processes.
///
/// The difficulty here is that process ids (PIDs) are not unique:
/// the kernel will reuse the ids after some time.
/// In case of a PID reuse, the file descriptor that pointed to the
/// old process is no longer valid. However, we pass the `Process`
/// (which contains the fd) to another task, and cannot check that
/// it still work in ProcessWatcher. `ProcessWatcher` implements
/// another heuristic.
///
/// See https://github.com/eminence/procfs/issues/125.
struct ProcessWatcher {
    watched_processes: HashMap<i32, ProcessFingerprint>,
    alumet_handle: ScopedControlHandle,
    ms_per_ticks: u64,

    // metrics
    metric_cpu_time: TypedMetricId<u64>,
    metric_memory: TypedMetricId<u64>,
}

#[derive(PartialEq, Eq)]
struct ProcessFingerprint {
    start_time: u64,
    ppid: i32,
}

impl ProcessFingerprint {
    fn new(stat: &procfs::process::Stat) -> Self {
        Self {
            start_time: stat.starttime,
            ppid: stat.ppid,
        }
    }
}

impl ProcessWatcher {
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        for p in procfs::process::all_processes().context("cannot read /proc")? {
            match p {
                Ok(process) => {
                    let pid = process.pid;
                    let stat = match process.stat() {
                        Err(ProcError::NotFound(_)) => continue, // process vanished, ignore
                        other => other,
                    }?;
                    let fingerprint = ProcessFingerprint::new(&stat);
                    let is_new = if let Some(existing) = self.watched_processes.get_mut(&pid) {
                        // A process with the same pid is in our HashMap.
                        // Is it the *same* process?
                        if existing == &fingerprint {
                            // same process, nothing to do
                            false
                        } else {
                            // new process! watch it
                            *existing = fingerprint;
                            true
                        }
                    } else {
                        self.watched_processes.insert(pid, fingerprint);
                        true
                    };
                    if is_new {
                        self.on_new_process(process)?;
                    }
                }
                Err(ProcError::NotFound(_)) => continue,
                Err(e) => Err(e)?,
            }
        }
        Ok(())
    }

    fn on_new_process(&mut self, p: Process) -> anyhow::Result<()> {
        let name = format!("pid-{}", p.pid);
        let trigger = todo!();
        let source = Box::new(
            ProcessStatsProbe::new(p, self.ms_per_ticks, self.metric_cpu_time, self.metric_memory)
                .with_context(|| format!("failed to create source {name}"))?,
        );
        self.alumet_handle
            .add_source(&name, source, trigger)
            .with_context(|| format!("failed to add source {name}"));
        Ok(())
    }
}
