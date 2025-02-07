//! Process-level (by pid) metrics.

use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Seek},
    time::Duration,
};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        control::{error::ControlError, ScopedControlHandle, SourceCreationBuffer},
        elements::{error::PollError, source},
        matching::{NamePattern, NamePatterns, SourceSelector},
        trigger::TriggerSpec,
        Source,
    },
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use procfs::{self, process::Process, FromRead, ProcError};
use regex::Regex;

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
    /// If true, push the first statistics (i.e. push even is `previous_general_stats` is empty).
    ///
    /// Useful when we measure a process that has just been started.
    push_first_stats: bool,

    // metrics
    metric_cpu_time: TypedMetricId<u64>,
    metric_memory: TypedMetricId<u64>,
}

impl ProcessStatsProbe {
    fn new(
        process: Process,
        ms_per_ticks: u64,
        push_first_stats: bool,
        metric_cpu_time: TypedMetricId<u64>,
        metric_memory: TypedMetricId<u64>,
    ) -> Result<Self, procfs::ProcError> {
        Ok(Self {
            pid: process.pid,
            ms_per_ticks,
            reader_stat: BufReader::new(process.open_relative("stat")?),
            reader_statm: BufReader::new(process.open_relative("statm")?),
            previous_general_stats: None,
            push_first_stats,
            metric_cpu_time,
            metric_memory,
        })
    }
}

fn stop_if_proc_not_found(err: ProcError) -> PollError {
    match err {
        ProcError::NotFound(_) => PollError::NormalStop,
        ProcError::Io(err, _) if err.raw_os_error() == Some(3) => {
            // "No such process" not caught by the procfs crate (it should ideally be mapped to ProcError::NotFound)
            PollError::NormalStop
        }
        _ => PollError::Fatal(err.into()),
    }
}

fn stop_if_io_not_found(err: io::Error) -> PollError {
    match err.kind() {
        io::ErrorKind::NotFound => PollError::NormalStop,
        _ if err.raw_os_error() == Some(3) => PollError::NormalStop,
        _ => PollError::Fatal(err.into()),
    }
}

impl Source for ProcessStatsProbe {
    fn poll(&mut self, buffer: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        let consumer = ResourceConsumer::Process { pid: self.pid as u32 };
        log::trace!("polled for consumer {consumer:?}");

        self.reader_stat.rewind().map_err(stop_if_io_not_found)?;
        self.reader_statm.rewind().map_err(stop_if_io_not_found)?;
        let general_stats = procfs::process::Stat::from_read(&mut self.reader_stat).map_err(stop_if_proc_not_found)?;
        let memory_stats = procfs::process::StatM::from_read(&mut self.reader_statm).map_err(stop_if_proc_not_found)?;

        // TODO how to report the state of the process in the timeseries?
        // let state = now.state()?;

        // Compute CPU usage in the last time slice.
        let cpu_usage = match self.previous_general_stats.take() {
            Some(prev) => Some(DeltaCpuTime::compute_diff(&prev, &general_stats, self.ms_per_ticks)),
            None if self.push_first_stats => Some(DeltaCpuTime::compute_first(&general_stats, self.ms_per_ticks)),
            None => None,
        };
        if let Some(measurements) = cpu_usage {
            measurements.push_measurements(
                self.metric_cpu_time,
                Resource::LocalMachine,
                consumer.clone(),
                buffer,
                t,
            );
        }
        self.previous_general_stats = Some(general_stats);

        // Compute RAM usage in the last time slice.
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory,
                Resource::LocalMachine,
                consumer.clone(),
                memory_stats.resident,
            )
            .with_attr("memory_kind", "resident"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory,
                Resource::LocalMachine,
                consumer.clone(),
                memory_stats.shared,
            )
            .with_attr("memory_kind", "shared"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory,
                Resource::LocalMachine,
                consumer,
                memory_stats.size,
            )
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
pub struct ProcessWatcher {
    watched_processes: HashMap<i32, ProcessFingerprint>,
    alumet_handle: ScopedControlHandle,
    monitoring: MultiProcessMonitoring,
}

/// Information required to setup the monitoring of individual processes.
struct MultiProcessMonitoring {
    source_spawner: ProcessSourceSpawner,
    groups: Vec<(ProcessFilter, MonitoringSettings)>,
}

pub struct ManualProcessMonitor {
    alumet_handle: ScopedControlHandle,
    source_spawner: ProcessSourceSpawner,
    settings: MonitoringSettings,
}

struct ProcessSourceSpawner {
    ms_per_ticks: u64,
    metrics: ProcessMetrics,
}

pub struct ProcessMetrics {
    pub metric_cpu_time: TypedMetricId<u64>,
    pub metric_memory: TypedMetricId<u64>,
}

#[derive(Debug)]
pub struct ProcessFilter {
    pub pid: Option<i32>,
    pub ppid: Option<i32>,
    pub exe_regex: Option<Regex>,
}

#[derive(Debug)]
pub struct MonitoringSettings {
    pub poll_interval: Duration,
    pub flush_interval: Duration,
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

impl ManualProcessMonitor {
    pub fn new(alumet_handle: ScopedControlHandle, metrics: ProcessMetrics, settings: MonitoringSettings) -> Self {
        let tps = procfs::ticks_per_second();
        let ms_per_ticks = 1000 / tps;
        Self {
            alumet_handle,
            source_spawner: ProcessSourceSpawner { ms_per_ticks, metrics },
            settings,
        }
    }

    pub fn start_monitoring(&self, pid: i32, source_buf: &mut SourceCreationBuffer) -> anyhow::Result<()> {
        let p = Process::new(pid).with_context(|| format!("could not acquire information about pid {pid}"))?;
        self.source_spawner.create_source_in(p, &self.settings, source_buf)?;
        // TODO find a more elegant way to immediately trigger a source that has just been created
        let source_name = format!("pid-{pid}");
        self.alumet_handle
            .anonymous()
            .try_send(alumet::pipeline::control::ControlMessage::Source(
                source::ControlMessage::TriggerManually(source::TriggerMessage {
                    selector: SourceSelector::from(NamePatterns {
                        plugin: NamePattern::Exact(String::from("procfs")),
                        name: NamePattern::Exact(source_name),
                    }),
                }),
            ))
            .map_err(ControlError::from)
            .context("failed to trigger the new source")?;
        Ok(())
    }
}

impl ProcessWatcher {
    pub fn new(
        alumet_handle: ScopedControlHandle,
        metrics: ProcessMetrics,
        groups: Vec<(ProcessFilter, MonitoringSettings)>,
    ) -> Self {
        let tps = procfs::ticks_per_second();
        let ms_per_ticks = 1000 / tps;
        Self {
            watched_processes: HashMap::new(),
            alumet_handle,
            monitoring: MultiProcessMonitoring {
                source_spawner: ProcessSourceSpawner { ms_per_ticks, metrics },
                groups,
            },
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        // The loop below can add a lot of sources, which can put too much pressure on the control packet buffer.
        // To avoid filling the buffer, we call `source_buffer()` and use the returned structure to group all the sources
        // creation messages together, in a single message that will be sent after the loop.
        let mut source_buf = self.alumet_handle.source_buffer();
        let monitoring = &mut self.monitoring;
        for p in procfs::process::all_processes().context("cannot read /proc")? {
            match p {
                Ok(process) => {
                    let pid = process.pid;
                    let stat = match process.stat() {
                        Err(ProcError::NotFound(_)) => continue, // process vanished, ignore
                        Err(ProcError::PermissionDenied(path)) => {
                            // permission denied, warn and skip
                            let path = path.unwrap_or_default();
                            let path = path.display();
                            log::warn!("cannot read process statistics from {path}: permission denied");
                            continue;
                        }
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
                        monitoring.on_new_process(process, &mut source_buf)?;
                    }
                }
                Err(ProcError::NotFound(_)) => continue,
                Err(e) => Err(e)?,
            }
        }
        // Flush the buffer and handle errors.
        // NOTE: the buffer is automatically flushed on drop, but calling `flush()` is better because it
        // allows to handle errors.
        source_buf
            .flush()
            .context("failed to create sources for new processes detected by ProcessWatcher")?;
        Ok(())
    }
}

impl MultiProcessMonitoring {
    fn on_new_process(&mut self, p: Process, source_buf: &mut SourceCreationBuffer) -> anyhow::Result<()> {
        // find a group whose filter accepts the process
        let pid = p.pid;
        for (filter, settings) in &self.groups {
            match filter.accepts(&p) {
                Ok(false) => (), // not accepted by this group filter, continue
                Ok(true) => {
                    log::trace!("process {pid} matches filter {filter:?} with settings {settings:?}");
                    self.source_spawner.create_source_in(p, settings, source_buf)?;
                    return Ok(());
                }
                Err(ProcError::PermissionDenied(path)) => {
                    let path = path.unwrap_or_default();
                    let path = path.display();
                    log::warn!("cannot apply filter {filter:?} on process {pid}: permission denied on {path}");
                    continue;
                }
                Err(e) => {
                    return Err(e).context(format!(
                        "error while trying to apply filter {filter:?} on process {pid}"
                    ));
                }
            }
        }
        log::trace!("No process filter matches pid {}", p.pid);
        Ok(())
    }
}

impl ProcessSourceSpawner {
    fn create_source_in(
        &self,
        p: Process,
        settings: &MonitoringSettings,
        source_buf: &mut SourceCreationBuffer,
    ) -> anyhow::Result<()> {
        let source_name = format!("pid-{}", p.pid);
        let trigger = TriggerSpec::builder(settings.poll_interval)
            .flush_interval(settings.flush_interval)
            .build()
            .with_context(|| {
                format!(
                    "error in TriggerSpec builder with settings {:?} for pid {}",
                    settings, p.pid
                )
            })?;
        log::trace!("adding source {source_name} with trigger specification {trigger:?}");
        let source = Box::new(
            ProcessStatsProbe::new(
                p,
                self.ms_per_ticks,
                true,
                self.metrics.metric_cpu_time,
                self.metrics.metric_memory,
            )
            .with_context(|| format!("failed to create source {source_name}"))?,
        );
        source_buf.add_source(&source_name, source, trigger);
        Ok(())
    }
}

impl Source for ProcessWatcher {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        // No measurement, only refresh here.
        // This is good to allow Alumet to control the refresh frequency, and to allow it to be changed at any time.
        self.refresh().map_err(PollError::Fatal)
    }
}

impl ProcessFilter {
    fn accepts(&self, p: &Process) -> Result<bool, ProcError> {
        if let Some(pid) = self.pid {
            if p.pid != pid {
                return Ok(false);
            }
        }

        if let Some(ppid) = self.ppid {
            if p.stat()?.ppid != ppid {
                return Ok(false);
            }
        }

        if let Some(r) = &self.exe_regex {
            // cf. https://unix.stackexchange.com/questions/629869/getting-the-executable-name-in-linux-from-proc-and-detect-if-its-truncated
            // TODO in case of PermissionDenied error, print a hint about ptrace access mode PTRACE_MODE_READ_FSCREDS (requires cap SYS_PTRACE or process of same user)
            if let Some(path_string) = p.exe()?.to_str() {
                if r.is_match(path_string) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
