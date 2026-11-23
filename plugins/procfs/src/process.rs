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
        Source,
        control::{
            PluginControlHandle,
            handle::OnBackgroundError,
            request::{self, MultiCreationRequestBuilder},
        },
        elements::{error::PollError, source::trigger::TriggerSpec},
        matching::{SourceNamePattern, StringPattern},
    },
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use procfs::{self, FromRead, ProcError, process::Process};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatsMode {
    Quick,
    SlowButAccurate,
}

/// Reads process stats from `/proc/<pid>` for some `<pid>`.
struct ProcessStatsProbe {
    /// The process id, as reported by the kernel.
    pid: i32,

    /// Nanosecond per tick (computed from the "ticks per second" reported by the kernel).
    ///
    /// Useful to compute meaningful time from tick-based values.
    ns_per_ticks: u64,

    /// A reader opened to `/proc/<pid>/stat`
    reader_stat: BufReader<File>,

    /// A reader opened to `/proc/<pid>/statm` or to `/proc/<pid>/smaps_rollup`
    reader_mem: BufReader<File>,

    mem_mode: MemoryStatsMode,

    /// The previously measured stats, to compute the difference.
    previous_general_stats: Option<(Timestamp, procfs::process::Stat)>,
    /// If true, push the first statistics (i.e. push even is `previous_general_stats` is empty).
    ///
    /// Useful when we measure a process that has just been started.
    push_first_stats: bool,

    // metrics
    metrics: ProcessMetrics,

    /// The memory page size, in bytes
    page_size: u64,

    n_cores: usize,
}

impl ProcessStatsProbe {
    fn new(
        process: Process,
        ns_per_ticks: u64,
        push_first_stats: bool,
        metrics: ProcessMetrics,
        mem_mode: MemoryStatsMode,
        n_cores: usize,
    ) -> Result<Self, procfs::ProcError> {
        let file_mem = match mem_mode {
            MemoryStatsMode::Quick => process.open_relative("statm")?,
            MemoryStatsMode::SlowButAccurate => process.open_relative("smaps_rollup")?,
        };

        Ok(Self {
            pid: process.pid,
            ns_per_ticks,
            reader_stat: BufReader::new(process.open_relative("stat")?),
            reader_mem: BufReader::new(file_mem),
            mem_mode,
            previous_general_stats: None,
            push_first_stats,
            metrics,
            page_size: procfs::page_size(),
            n_cores,
        })
    }

    fn pages_to_bytes(&self, pages: u64) -> u64 {
        pages * self.page_size
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

struct MemoryStats {
    virtual_bytes: u64,
    resident_bytes: u64,
    shared_bytes: u64,
}

impl Source for ProcessStatsProbe {
    fn poll(&mut self, buffer: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        let consumer = ResourceConsumer::Process { pid: self.pid as u32 };
        log::trace!("polled for consumer {consumer:?}");

        self.reader_stat.rewind().map_err(stop_if_io_not_found)?;
        self.reader_mem.rewind().map_err(stop_if_io_not_found)?;
        let general_stats = procfs::process::Stat::from_read(&mut self.reader_stat).map_err(stop_if_proc_not_found)?;

        // we have two ways of computing memory usage
        let memory_stats = match self.mem_mode {
            MemoryStatsMode::Quick => {
                let statm = procfs::process::StatM::from_read(&mut self.reader_mem).map_err(stop_if_proc_not_found)?;
                MemoryStats {
                    virtual_bytes: self.pages_to_bytes(statm.size),
                    resident_bytes: self.pages_to_bytes(statm.resident),
                    shared_bytes: self.pages_to_bytes(statm.shared),
                }
            }
            MemoryStatsMode::SlowButAccurate => {
                let smaps_rollup =
                    procfs::process::SmapsRollup::from_read(&mut self.reader_mem).map_err(stop_if_proc_not_found)?;
                let smaps_rollup = &smaps_rollup
                    .memory_map_rollup
                    .0
                    .first()
                    .context("smaps_rollup returned no data")?
                    .extension
                    .map;
                let rss = smaps_rollup
                    .get("Rss")
                    .context("smaps_rollup did not contain field Rss")?;
                let shared_clean = smaps_rollup
                    .get("Shared_Clean")
                    .context("smaps_rollup did not contain field Shared_Clean")?;
                let shared_dirty = smaps_rollup
                    .get("Shared_Dirty")
                    .context("smaps_rollup did not contain field Shared_Dirty")?;
                let pss = smaps_rollup
                    .get("Pss")
                    .context("smaps_rollup did not contain field Pss")?;

                // push pss on its own, it has no equivalent in /proc/<pid>/statm
                buffer.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.metric_memory_usage,
                        Resource::LocalMachine,
                        consumer.clone(),
                        *pss,
                    )
                    .with_attr("kind", "proportional"),
                );

                // common memory stats
                MemoryStats {
                    virtual_bytes: general_stats.vsize, // smaps_rollup does not provide the vm size, but we have it from /proc/<pid>/stat
                    resident_bytes: *rss,               // already in bytes (converted by the procfs crate)
                    shared_bytes: shared_clean + shared_dirty, // already in bytes (converted by the procfs crate)
                }
            }
        };
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metrics.metric_memory_usage,
                Resource::LocalMachine,
                consumer.clone(),
                memory_stats.resident_bytes,
            )
            .with_attr("kind", "resident"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metrics.metric_memory_usage,
                Resource::LocalMachine,
                consumer.clone(),
                memory_stats.shared_bytes,
            )
            .with_attr("kind", "shared"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metrics.metric_memory_usage,
                Resource::LocalMachine,
                consumer.clone(),
                memory_stats.virtual_bytes,
            )
            .with_attr("kind", "virtual"),
        );

        // TODO how to report the state of the process in the timeseries?
        // let state = now.state()?;

        // Compute CPU usage in the last time slice.
        let (prev_t, cpu_usage) = match self.previous_general_stats.take() {
            Some((prev_t, prev_stat)) => (
                Some(prev_t),
                Some(DeltaCpuTime::compute_diff(
                    &prev_stat,
                    &general_stats,
                    self.ns_per_ticks,
                )),
            ),
            None if self.push_first_stats => (
                None,
                Some(DeltaCpuTime::compute_first(&general_stats, self.ns_per_ticks)),
            ),
            None => (None, None),
        };
        if let Some(measurements) = cpu_usage {
            measurements.push_cpu_measurements(
                self.metrics.metric_cpu_time_delta,
                Resource::LocalMachine,
                consumer.clone(),
                buffer,
                t,
            );
            if let Some(prev_t) = prev_t {
                let delta_t = t.duration_since(prev_t).unwrap().as_nanos() as f64;
                let n_cores = self.n_cores as f64;

                let user = measurements.user as f64;
                let system = measurements.system as f64;
                let total = user + system;

                let percent_user = 100.0 * (user / delta_t) / n_cores;
                let percent_system = 100.0 * (system / delta_t) / n_cores;
                let percent_total = 100.0 * (total / delta_t) / n_cores;
                // TODO guest value?
                buffer.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.metric_cpu_percent,
                        Resource::LocalMachine,
                        consumer.clone(),
                        percent_user,
                    )
                    .with_attr("kind", "user"),
                );
                buffer.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.metric_cpu_percent,
                        Resource::LocalMachine,
                        consumer.clone(),
                        percent_system,
                    )
                    .with_attr("kind", "system"),
                );
                buffer.push(
                    MeasurementPoint::new(
                        t,
                        self.metrics.metric_cpu_percent,
                        Resource::LocalMachine,
                        consumer.clone(),
                        percent_total,
                    )
                    .with_attr("kind", "total"),
                );
            }
        }
        self.previous_general_stats = Some((t, general_stats));
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
    pub fn compute_first(now: &procfs::process::Stat, ns_per_ticks: u64) -> Self {
        Self {
            user: now.utime * ns_per_ticks,
            system: now.stime * ns_per_ticks,
            guest: now.guest_time.map(|x| x * ns_per_ticks),
        }
    }

    pub fn compute_diff(prev: &procfs::process::Stat, now: &procfs::process::Stat, ns_per_ticks: u64) -> Self {
        Self {
            user: (now.utime - prev.utime) * ns_per_ticks,
            system: (now.stime - prev.stime) * ns_per_ticks,
            // children_user: (now.cutime - prev.cutime) * ns_per_ticks,
            // children_system: (now.cstime - prev.cstime) * ns_per_ticks,
            guest: now.guest_time.map(|x| (x - prev.guest_time.unwrap()) * ns_per_ticks),
            // children_guest: (now.cguest_time - prev.cguest_time) * ns_per_ticks,
        }
    }

    pub fn push_cpu_measurements(
        &self,
        metric: TypedMetricId<u64>,
        res: Resource,
        consumer: ResourceConsumer,
        acc: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) {
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.user)
                .with_attr("kind", "user"),
        );
        acc.push(
            MeasurementPoint::new(timestamp, metric, res.clone(), consumer.clone(), self.system)
                .with_attr("kind", "system"),
        );
        if let Some(guest) = self.guest {
            acc.push(MeasurementPoint::new(timestamp, metric, res, consumer, guest).with_attr("kind", "guest"));
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
    alumet_handle: PluginControlHandle,
    monitoring: MultiProcessMonitoring,
}

/// Information required to setup the monitoring of individual processes.
struct MultiProcessMonitoring {
    source_spawner: ProcessSourceSpawner,
    groups: Vec<(ProcessFilter, MonitoringSettings)>,
}

pub struct ManualProcessMonitor {
    alumet_handle: PluginControlHandle,
    source_spawner: ProcessSourceSpawner,
    settings: MonitoringSettings,
}

struct ProcessSourceSpawner {
    ns_per_ticks: u64,
    metrics: ProcessMetrics,
    n_cores: usize,
}

#[derive(Clone)]
pub struct ProcessMetrics {
    pub metric_cpu_time_delta: TypedMetricId<u64>,
    pub metric_cpu_percent: TypedMetricId<f64>,
    pub metric_memory_usage: TypedMetricId<u64>,
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
    pub mem_mode: MemoryStatsMode,
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
    pub fn new(alumet_handle: PluginControlHandle, metrics: ProcessMetrics, settings: MonitoringSettings) -> Self {
        // To get the number of logical core, one could think about calling num_cpus::get().
        // However, this is affected by the constraints set on the Alumet process (sched affinity, cgroups cpuset), which is not what we want.
        let n_cores = crate::cpus::online_cpus().unwrap_or_default().len();
        let ns_per_ticks = ns_per_ticks();
        Self {
            alumet_handle,
            source_spawner: ProcessSourceSpawner {
                ns_per_ticks,
                metrics,
                n_cores,
            },
            settings,
        }
    }

    pub fn start_monitoring(
        &self,
        pids: impl Iterator<Item = i32>,
        ctx: &tokio::runtime::Handle,
    ) -> anyhow::Result<()> {
        // prepare to create the sources
        let mut matchers = Vec::new();
        let mut request_builder = request::create_many();
        for pid in pids {
            log::debug!("Starting to monitor process with pid {pid}");
            let p = Process::new(pid).with_context(|| format!("could not acquire information about pid {pid}"))?;
            self.source_spawner
                .create_source_in(p, &self.settings, &mut request_builder)?;
            let source_name = format!("pid-{pid}");
            let matcher = SourceNamePattern::new(
                StringPattern::Exact(String::from("procfs")),
                StringPattern::Exact(source_name),
            );
            matchers.push(matcher);
        }
        let create_request = request_builder.build();

        // prepare to trigger the sources
        // TODO find a more elegant way to immediately trigger a source that has just been created
        let trigger_requests = matchers.into_iter().map(|m| request::source(m).trigger_now());

        // send requests
        ctx.block_on(async {
            self.alumet_handle
                .dispatch(create_request, Duration::from_secs(2))
                .await?;
            for req in trigger_requests {
                self.alumet_handle.dispatch(req, Duration::from_secs(1)).await?;
            }
            anyhow::Ok(())
        })
        .context("failed to dispatch messages")?;
        Ok(())
    }
}

impl ProcessWatcher {
    pub fn new(
        alumet_handle: PluginControlHandle,
        metrics: ProcessMetrics,
        groups: Vec<(ProcessFilter, MonitoringSettings)>,
    ) -> Self {
        // To get the number of logical core, one could think about calling num_cpus::get().
        // However, this is affected by the constraints set on the Alumet process (sched affinity, cgroups cpuset), which is not what we want.
        let n_cores = crate::cpus::online_cpus().unwrap_or_default().len();
        let ns_per_ticks = ns_per_ticks();
        Self {
            watched_processes: HashMap::new(),
            alumet_handle,
            monitoring: MultiProcessMonitoring {
                source_spawner: ProcessSourceSpawner {
                    ns_per_ticks,
                    metrics,
                    n_cores,
                },
                groups,
            },
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        // The loop below can add a lot of sources, which can put too much pressure on the control packet buffer.
        // To avoid filling the buffer, we group all the sources creation messages together
        // in a single message that will be sent after the loop.
        let mut request_builder = request::create_many();
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
                        monitoring.on_new_process(process, &mut request_builder)?;
                    }
                }
                Err(ProcError::NotFound(_)) => continue,
                Err(e) => Err(e)?,
            }
        }
        // Send the request and handle errors.
        let request = request_builder.build();

        // Sources run in a tokio context, we can use the underlying runtime.
        self.alumet_handle
            .dispatch_in_current_runtime(request, None, OnBackgroundError::Log)
            .context("failed to dispatch request from ProcessWatcher")?;
        Ok(())
    }
}

impl MultiProcessMonitoring {
    fn on_new_process(&mut self, p: Process, request_builder: &mut MultiCreationRequestBuilder) -> anyhow::Result<()> {
        // find a group whose filter accepts the process
        let pid = p.pid;
        for (filter, settings) in &self.groups {
            match filter.accepts(&p) {
                Ok(false) => (), // not accepted by this group filter, continue
                Ok(true) => {
                    log::trace!("process {pid} matches filter {filter:?} with settings {settings:?}");
                    self.source_spawner.create_source_in(p, settings, request_builder)?;
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
        create_many: &mut MultiCreationRequestBuilder,
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
                self.ns_per_ticks,
                true,
                self.metrics.clone(),
                settings.mem_mode,
                self.n_cores,
            )
            .with_context(|| format!("failed to create source {source_name}"))?,
        );
        create_many.add_source(&source_name, source, trigger);
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
        if let Some(pid) = self.pid
            && p.pid != pid
        {
            return Ok(false);
        }

        if let Some(ppid) = self.ppid
            && p.stat()?.ppid != ppid
        {
            return Ok(false);
        }

        if let Some(r) = &self.exe_regex {
            // cf. https://unix.stackexchange.com/questions/629869/getting-the-executable-name-in-linux-from-proc-and-detect-if-its-truncated
            // TODO in case of PermissionDenied error, print a hint about ptrace access mode PTRACE_MODE_READ_FSCREDS (requires cap SYS_PTRACE or process of same user)
            if let Some(path_string) = p.exe()?.to_str() {
                if !r.is_match(path_string) {
                    return Ok(false);
                }
            } else {
                // not valid utf-8
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn ns_per_ticks() -> u64 {
    1_000_000_000 / procfs::ticks_per_second()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PPID: usize = 4;
    const UTIME: usize = 14;
    const STIME: usize = 15;
    const STARTTIME: usize = 22;
    const GUEST_TIME: usize = 43;

    /// Builds a `/proc/<pid>/stat` with `n_fields` fields. The given fields are identified
    /// by their 1-based index, as in `man 5 proc`; the others are set to 0.
    fn stat(fields: &[(usize, u64)], n_fields: usize) -> procfs::process::Stat {
        let mut line = String::from("42 (test) S");
        for i in 4..=n_fields {
            let value = fields.iter().find(|(idx, _)| *idx == i).map_or(0, |(_, v)| *v);
            line.push_str(&format!(" {value}"));
        }
        procfs::process::Stat::from_read(io::Cursor::new(line)).unwrap()
    }

    mod delta_cpu_time {
        use super::*;

        /// Without a previous measurement, the whole cpu time is converted from ticks to ns.
        #[test]
        fn first() {
            let d = DeltaCpuTime::compute_first(&stat(&[(UTIME, 10), (STIME, 4), (GUEST_TIME, 2)], 52), 1000);
            assert_eq!((d.user, d.system, d.guest), (10_000, 4_000, Some(2_000)));
        }

        /// The guest time is `None` when the kernel does not provide it.
        #[test]
        fn first_without_guest() {
            let d = DeltaCpuTime::compute_first(&stat(&[(UTIME, 10)], 41), 1000);
            assert_eq!(d.guest, None);
        }

        /// The cpu time is the difference between two measurements, converted from ticks to ns.
        #[test]
        fn diff() {
            let prev = stat(&[(UTIME, 10), (STIME, 4), (GUEST_TIME, 2)], 52);
            let now = stat(&[(UTIME, 25), (STIME, 5), (GUEST_TIME, 3)], 52);
            let d = DeltaCpuTime::compute_diff(&prev, &now, 10);
            assert_eq!((d.user, d.system, d.guest), (150, 10, Some(10)));
        }

        /// The difference works when the kernel does not provide the guest time.
        #[test]
        fn diff_without_guest() {
            let d = DeltaCpuTime::compute_diff(&stat(&[(UTIME, 1)], 41), &stat(&[(UTIME, 3)], 41), 7);
            assert_eq!((d.user, d.system, d.guest), (14, 0, None));
        }
    }

    mod process_fingerprint {
        use super::*;

        fn fingerprint(ppid: u64, starttime: u64) -> ProcessFingerprint {
            ProcessFingerprint::new(&stat(&[(PPID, ppid), (STARTTIME, starttime)], 52))
        }

        /// Two measurements of the same process have the same fingerprint.
        #[test]
        fn same_process() {
            assert!(fingerprint(1, 100) == fingerprint(1, 100));
        }

        /// A reused pid with another start time is a different process.
        #[test]
        fn other_start_time() {
            assert!(fingerprint(1, 100) != fingerprint(1, 200));
        }

        /// A reused pid with another parent is a different process.
        #[test]
        fn other_parent() {
            assert!(fingerprint(1, 100) != fingerprint(2, 100));
        }
    }

    mod stop_if_proc_not_found {
        use super::*;

        /// A process that disappeared stops the source normally.
        #[test]
        fn not_found() {
            let res = stop_if_proc_not_found(ProcError::NotFound(None));
            assert!(matches!(res, PollError::NormalStop));
        }

        /// ESRCH ("No such process") stops the source normally.
        #[test]
        fn no_such_process() {
            let res = stop_if_proc_not_found(ProcError::Io(io::Error::from_raw_os_error(3), None));
            assert!(matches!(res, PollError::NormalStop));
        }

        /// Other errors are fatal.
        #[test]
        fn other_error() {
            let res = stop_if_proc_not_found(ProcError::PermissionDenied(None));
            assert!(matches!(res, PollError::Fatal(_)));
        }
    }

    mod stop_if_io_not_found {
        use super::*;

        /// A file that disappeared stops the source normally.
        #[test]
        fn not_found() {
            let res = stop_if_io_not_found(io::ErrorKind::NotFound.into());
            assert!(matches!(res, PollError::NormalStop));
        }

        /// ESRCH ("No such process") stops the source normally.
        #[test]
        fn no_such_process() {
            let res = stop_if_io_not_found(io::Error::from_raw_os_error(3));
            assert!(matches!(res, PollError::NormalStop));
        }

        /// Other I/O errors are fatal.
        #[test]
        fn other_error() {
            let res = stop_if_io_not_found(io::ErrorKind::PermissionDenied.into());
            assert!(matches!(res, PollError::Fatal(_)));
        }
    }

    mod ns_per_ticks {
        use super::*;

        /// The number of ns per tick matches the tick rate of the kernel.
        #[test]
        fn one_second() {
            assert_eq!(ns_per_ticks() * procfs::ticks_per_second(), 1_000_000_000);
        }
    }

    mod process_filter {
        use super::*;

        fn accepts(pid: Option<i32>, ppid: Option<i32>, exe_regex: Option<&str>) -> bool {
            let filter = ProcessFilter {
                pid,
                ppid,
                exe_regex: exe_regex.map(|r| Regex::new(r).unwrap()),
            };
            filter.accepts(&Process::myself().unwrap()).unwrap()
        }

        fn my_pid() -> i32 {
            std::process::id() as i32
        }

        fn my_ppid() -> i32 {
            std::os::unix::process::parent_id() as i32
        }

        /// A filter without criteria accepts any process.
        #[test]
        fn empty() {
            assert!(accepts(None, None, None));
        }

        /// The pid filter only accepts the process with this pid.
        #[test]
        fn pid() {
            assert!(accepts(Some(my_pid()), None, None));
            assert!(!accepts(Some(my_pid() + 1), None, None));
        }

        /// The ppid filter only accepts the children of this parent.
        #[test]
        fn ppid() {
            assert!(accepts(None, Some(my_ppid()), None));
            assert!(!accepts(None, Some(my_ppid() + 1), None));
        }

        /// The regex filter is applied to the path of the executable.
        #[test]
        fn exe_regex() {
            let exe = std::env::current_exe().unwrap();
            let name = regex::escape(exe.file_name().unwrap().to_str().unwrap());
            assert!(accepts(None, None, Some(&format!("{name}$"))));
            assert!(!accepts(None, None, Some("^not-the-exe$")));
        }

        /// When several criteria are given, all of them must match.
        #[test]
        fn all_criteria() {
            assert!(accepts(Some(my_pid()), Some(my_ppid()), Some(".*")));
            assert!(!accepts(Some(my_pid()), Some(my_ppid()), Some("^nope$")));
        }
    }
}
