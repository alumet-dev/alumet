use rlimit::{Resource, getrlimit, setrlimit};
use std::{
    collections::HashMap,
    fs::File,
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    metrics::TypedMetricId,
    pipeline::{
        Source,
        control::{
            PluginControlHandle,
            matching::SourceMatcher,
            request::{self, CreationRequest, MultiCreationRequestBuilder},
        },
        elements::source::{control::TaskState, trigger::TriggerSpec},
        matching::{SourceNamePattern, StringPattern},
    },
    plugin::{
        AlumetPostStart, event as alumet_event_bus,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use anyhow::Context;

use crate::counters::{GroupedEvent, PerfEventTarget};
use crate::group::OpenError;
use alumet::resources::ResourceConsumer;
use cgroup_via_processes::CgroupProcessesSource;
use config::{CgroupCounting, Config, ParsedConfig, sanitize};
use lazy_source::{LazySource, SourceFactory};
use source::PerfEventSource;

mod cgroup_via_processes;
mod config;
mod lazy_source;
mod source;

pub struct PerfPlugin {
    /// The events parsed in `init`, until `start` creates their metric.
    events: Vec<GroupedEvent>,
    config: Arc<Mutex<ParsedConfig>>,
}

impl AlumetPlugin for PerfPlugin {
    fn name() -> &'static str {
        "perf"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        // Parse the perf events with the unified syntax. One entry may expand to several events (a
        // generic event fanned onto every core PMU of a hybrid CPU).
        let mut events = Vec::with_capacity(config.events.len());
        for entry in &config.events {
            events.extend(entry.parse().context("invalid event in config")?);
        }
        let config = ParsedConfig {
            // Store the source settings.
            poll_interval: config.poll_interval,
            flush_interval: config.flush_interval,
            events: Vec::new(),
            multiplexing_auto_scale: config.multiplexing_auto_scale,
            add_source_in_pause_state: config.add_source_in_pause_state,
            cgroup_via_processes: match config.cgroup_counting {
                CgroupCounting::Native => None,
                CgroupCounting::ViaProcesses { scan_interval } => Some(scan_interval),
            },
        };
        Ok(Box::new(PerfPlugin {
            events,
            config: Arc::new(Mutex::new(config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        increase_file_descriptors_soft_limit().context("error while increasing file descriptors soft limit")?;

        let mut config = self.config.lock().unwrap();

        // Events fanned onto several core PMUs share one metric (same name, told apart by their
        // `pmu` attribute), so a metric is created once per distinct name and reused.
        let mut by_name: HashMap<String, TypedMetricId<u64>> = HashMap::new();
        for grouped in std::mem::take(&mut self.events) {
            let e = &grouped.event;
            let metric_name = format!("perf_{}", sanitize(e.name()));
            let metric = match by_name.get(&metric_name) {
                Some(metric) => *metric,
                None => {
                    let metric = alumet.create_metric::<u64>(&metric_name, Unit::Unity, e.description().to_owned())?;
                    by_name.insert(metric_name, metric);
                    metric
                }
            };
            config.events.push((grouped, metric));
        }
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        const REQUEST_TIMEOUT: Duration = Duration::from_secs(1);
        let config = self.config.lock().unwrap().clone();
        let factory = PerfSourceFactory::new(config)?;

        // Unlike the per-process/cgroup sources below, the system-wide is not reactive: it exists for the whole machine, so
        // it is created once, rather than when a consumer appears.
        match factory.create_system_source() {
            Ok(None) => (),
            Ok(Some(request)) => {
                alumet.block_on(alumet.pipeline_control().dispatch(request, REQUEST_TIMEOUT))?;
            }
            // Not fatal: system-wide PMUs need CAP_PERFMON / perf_event_paranoid < 1, which
            // the per-process/cgroup events do not. Keep the rest of the plugin working.
            Err(e) => log::warn!("{e:#}; continuing without them"),
        }

        // Listen to start consumer events, starting sources.
        let runtime = alumet.async_runtime();
        let pipeline_control = alumet.pipeline_control();
        alumet_event_bus::start_consumer_measurement().subscribe(move |e| {
            let mut multi_request = request::create_many();
            for consumer in e.0 {
                factory.create_consumer_source(consumer, &mut multi_request);
            }
            runtime.block_on(pipeline_control.dispatch(multi_request.build(), REQUEST_TIMEOUT))?;
            Ok(())
        });

        // Listen to end consumer events, stopping sources.
        let runtime = alumet.async_runtime();
        let pipeline_control = alumet.pipeline_control();
        alumet_event_bus::end_consumer_measurement().subscribe(move |e| {
            for consumer in e.0 {
                let source_name = match consumer {
                    alumet::resources::ResourceConsumer::Process { pid } => process_source_name(pid),
                    alumet::resources::ResourceConsumer::ControlGroup { path } => cgroup_source_name(path),
                    _ => continue,
                };
                let stop_request = request::source::source(SourceMatcher::Name(SourceNamePattern::new(
                    StringPattern::Exact("perf".to_string()),
                    StringPattern::Exact(source_name.clone()),
                )))
                .stop();
                runtime.block_on(pipeline_control.dispatch(stop_request, REQUEST_TIMEOUT))?;
                log::debug!("Source {source_name} has been requested to stop.");
            }
            Ok(())
        });

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct PerfSourceFactory {
    config: ParsedConfig,
    trigger: TriggerSpec,
}

impl PerfSourceFactory {
    fn new(config: ParsedConfig) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::builder(config.poll_interval)
            .flush_interval(config.flush_interval)
            .build()?;
        Ok(Self { config, trigger })
    }

    /// Builds an Alumet control request that creates the system-wide source for system-wide events (uncore, power, cstate…).
    fn create_system_source(&self) -> anyhow::Result<Option<CreationRequest>> {
        let system_events = self.config.events(crate::event::ScopeKind::SystemWide);
        if system_events.is_empty() {
            return Ok(None);
        }
        let n = system_events.len();
        let init_source_state = match self.config.add_source_in_pause_state {
            true => TaskState::Pause,
            false => TaskState::Run,
        };

        let auto_scale = self.config.multiplexing_auto_scale;
        let source = self.build_source(Box::new(move || {
            let source = PerfEventSource::new(
                &PerfEventTarget::Machine,
                &system_events,
                auto_scale,
                ResourceConsumer::LocalMachine,
            )
            .map_err(|e| e.context("could not open system-wide perf events (uncore/power/cstate)".to_owned()))?;
            Ok(Box::new(source))
        }))?;

        let trigger = self.trigger.clone();
        let request =
            request::create_one().add_source_with_state("source-system-wide", source, trigger, init_source_state);
        log::info!("Prepared system-wide perf source with {n} event(s).");

        Ok(Some(request))
    }

    /// Opens the source now, or on its first poll if it starts paused: it may never be resumed,
    /// and its counters would count (and multiplex) for nothing meanwhile.
    fn build_source(&self, mut open: SourceFactory) -> anyhow::Result<Box<dyn Source>> {
        if self.config.add_source_in_pause_state {
            Ok(Box::new(LazySource::new(open)))
        } else {
            Ok(open()?)
        }
    }

    fn create_consumer_source(&self, consumer: ResourceConsumer, requests: &mut MultiCreationRequestBuilder) -> () {
        let consumer_events = self.config.events(crate::event::ScopeKind::TaskAttached);
        if consumer_events.is_empty() {
            return;
        }
        let auto_scale = self.config.multiplexing_auto_scale;
        let (open, source_name): (SourceFactory, String) = match consumer.clone() {
            ResourceConsumer::Process { pid } => {
                let open: SourceFactory = Box::new(move || {
                    // The main thread of the process, whose tid is the pid.
                    let observed = PerfEventTarget::Task {
                        tid: pid,
                        inherit: false,
                    };
                    log::info!("Starting to observe process {pid}...");
                    let source = PerfEventSource::new(&observed, &consumer_events, auto_scale, consumer.clone())
                        .map_err(|e| {
                            e.context(format!("cannot observe events {consumer_events:?} on process {pid}"))
                        })?;
                    Ok(Box::new(source))
                });
                (open, process_source_name(pid))
            }
            ResourceConsumer::ControlGroup { path } => {
                // making an assumption about the cgroup mounting point here to be /sys/fs/cgroup
                // we just have information about the canonical path here
                // making it hard to not recompute the mounting path here
                // note that it will only work for cgroup v2
                // TODO: make it dynamic or configurable
                let absolute_path = format!("/sys/fs/cgroup{path}");
                let via_processes = self.config.cgroup_via_processes;
                let name = cgroup_source_name(path.clone());
                let open: SourceFactory = Box::new(move || {
                    if let Some(scan_interval) = via_processes {
                        // Workaround for the kernels that miscount multiplexed cgroup events.
                        let source = CgroupProcessesSource::new(
                            PathBuf::from(&absolute_path),
                            &consumer_events,
                            auto_scale,
                            consumer.clone(),
                            scan_interval,
                        )
                        .with_context(|| format!("cannot observe cgroup {absolute_path}"))?;
                        return Ok(Box::new(source));
                    }
                    log::info!("Starting to observe cgroup {path}...");
                    let fd = match File::open(&absolute_path) {
                        Ok(fd) => fd,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(OpenError::TargetGone(e)),
                        Err(e) => {
                            let e = anyhow::Error::new(e).context(format!("cgroup not readable: {absolute_path}"));
                            return Err(OpenError::Other(e));
                        }
                    };
                    let observed = PerfEventTarget::Cgroup { fd: Arc::new(fd) };
                    let source = PerfEventSource::new(&observed, &consumer_events, auto_scale, consumer.clone())
                        .map_err(|e| {
                            e.context(format!(
                                "cannot observe events {consumer_events:?} on cgroup {absolute_path}"
                            ))
                        })?;
                    Ok(Box::new(source))
                });
                (open, name)
            }
            _ => return,
        };
        let source = match self.build_source(open) {
            Ok(source) => source,
            Err(e) => {
                log::warn!("{source_name}: {e:#}");
                return;
            }
        };
        let trigger = self.trigger.clone();
        let init_source_state = match self.config.add_source_in_pause_state {
            false => TaskState::Run,
            true => TaskState::Pause,
        };
        requests.add_source_with_state(&source_name, source, trigger, init_source_state);
        log::info!("New source prepared: {source_name}");
    }
}

// prevent 'Too many open files' error
fn increase_file_descriptors_soft_limit() -> Result<(), anyhow::Error> {
    let (fd_soft, fd_hard) = getrlimit(Resource::NOFILE).context("Error while getting file descriptors limits")?;
    setrlimit(Resource::NOFILE, fd_hard, fd_hard)
        .with_context(|| format!("Error while setting file descriptors soft limit from {fd_soft} to {fd_hard}"))?;
    log::debug!(
        "Increased file descriptors soft limit ({fd_soft}) to reach hard limit value ({fd_hard}) to prevent 'Too many open files' error"
    );
    Ok(())
}

fn process_source_name(pid: u32) -> String {
    format!("source-pid[{pid}]")
}

fn cgroup_source_name(path: alumet::resources::StrCow) -> String {
    format!("source-cgroup[{path}]")
}
