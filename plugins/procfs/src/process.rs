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
    /// A reader opened to `/proc/<pid>/statm`
    reader_statm: BufReader<File>,

    /// The previously measured stats, to compute the difference.
    previous_general_stats: Option<procfs::process::Stat>,
    /// If true, push the first statistics (i.e. push even is `previous_general_stats` is empty).
    ///
    /// Useful when we measure a process that has just been started.
    push_first_stats: bool,

    // metrics
    metric_cpu_time_delta: TypedMetricId<u64>,
    metric_memory_usage: TypedMetricId<u64>,

    /// The memory page size, in bytes
    page_size: u64,
}

impl ProcessStatsProbe {
    fn new(
        process: Process,
        ns_per_ticks: u64,
        push_first_stats: bool,
        metric_cpu_time_delta: TypedMetricId<u64>,
        metric_memory_usage: TypedMetricId<u64>,
    ) -> Result<Self, procfs::ProcError> {
        Ok(Self {
            pid: process.pid,
            ns_per_ticks,
            reader_stat: BufReader::new(process.open_relative("stat")?),
            reader_statm: BufReader::new(process.open_relative("statm")?),
            previous_general_stats: None,
            push_first_stats,
            metric_cpu_time_delta,
            metric_memory_usage,
            page_size: procfs::page_size(),
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
            Some(prev) => Some(DeltaCpuTime::compute_diff(&prev, &general_stats, self.ns_per_ticks)),
            None if self.push_first_stats => Some(DeltaCpuTime::compute_first(&general_stats, self.ns_per_ticks)),
            None => None,
        };
        if let Some(measurements) = cpu_usage {
            measurements.push_cpu_measurements(
                self.metric_cpu_time_delta,
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
                self.metric_memory_usage,
                Resource::LocalMachine,
                consumer.clone(),
                self.pages_to_bytes(memory_stats.resident),
            )
            .with_attr("kind", "resident"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory_usage,
                Resource::LocalMachine,
                consumer.clone(),
                self.pages_to_bytes(memory_stats.shared),
            )
            .with_attr("kind", "shared"),
        );
        buffer.push(
            MeasurementPoint::new(
                t,
                self.metric_memory_usage,
                Resource::LocalMachine,
                consumer,
                self.pages_to_bytes(memory_stats.size),
            )
            .with_attr("kind", "virtual"),
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
}

pub struct ProcessMetrics {
    pub metric_cpu_time_delta: TypedMetricId<u64>,
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
        let tps = procfs::ticks_per_second();
        let ns_per_ticks = ns_per_ticks();
        Self {
            alumet_handle,
            source_spawner: ProcessSourceSpawner { ns_per_ticks, metrics },
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
        let tps = procfs::ticks_per_second();
        let ns_per_ticks = ns_per_ticks();
        Self {
            watched_processes: HashMap::new(),
            alumet_handle,
            monitoring: MultiProcessMonitoring {
                source_spawner: ProcessSourceSpawner { ns_per_ticks, metrics },
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
                self.metrics.metric_cpu_time_delta,
                self.metrics.metric_memory_usage,
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

fn ns_per_ticks() -> u64 {
    1_000_000_000 / procfs::ticks_per_second()
}
