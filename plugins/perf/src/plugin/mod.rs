use rlimit::{Resource, getrlimit, setrlimit};
use std::{
    collections::HashMap,
    fs::File,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    metrics::TypedMetricId,
    pipeline::{
        Source,
        control::{matching::SourceMatcher, request},
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

use crate::counters::{GroupedEvent, Observed};
use crate::event;
use alumet::resources::ResourceConsumer;
use cgroup_via_processes::CgroupProcessesSource;
use config::{CgroupCounting, Config, ParsedConfig, sanitize};
use source::PerfEventSource;

mod cgroup_via_processes;
mod config;
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
        increase_file_descriptors_soft_limit().context("Error while increasing file descriptors soft limit")?;

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
        let config_cloned = self.config.clone();
        let pipeline_control_start = alumet.pipeline_control();
        let pipeline_control_end = alumet.pipeline_control();
        let runtime_start = alumet.async_runtime().clone();
        let runtime_end = alumet.async_runtime().clone();

        // Start the machine-wide source for system-wide events (uncore, power, cstate…). Unlike the
        // per-process/cgroup sources below, it is not reactive: it exists for the whole machine, so
        // it is created once, here, rather than when a consumer appears.
        {
            let config = self.config.lock().unwrap();
            let system_events = events_of(&config, true);
            if !system_events.is_empty() {
                let n = system_events.len();
                let poll_interval = config.poll_interval;
                let flush_interval = config.flush_interval;
                let auto_scale = config.multiplexing_auto_scale;
                let add_source_in_pause_state = config.add_source_in_pause_state;
                drop(config);

                let init_source_state = match add_source_in_pause_state {
                    false => TaskState::Run,
                    true => TaskState::Pause,
                };

                let source = PerfEventSource::new(
                    &Observed::Machine,
                    &system_events,
                    auto_scale,
                    ResourceConsumer::LocalMachine,
                );
                match source {
                    Ok(source) => {
                        let trigger = TriggerSpec::builder(poll_interval)
                            .flush_interval(flush_interval)
                            .build()?;
                        let request = request::create_one().add_source_with_state(
                            "source-system-wide",
                            Box::new(source),
                            trigger,
                            init_source_state,
                        );
                        runtime_start.block_on(pipeline_control_start.dispatch(request, Duration::from_secs(1)))?;
                        log::info!("Started system-wide perf source with {n} event(s).");
                    }
                    // Not fatal: system-wide PMUs need CAP_PERFMON / perf_event_paranoid < 1, which
                    // the per-process/cgroup events do not. Keep the rest of the plugin working.
                    Err(e) => log::warn!(
                        "could not open system-wide perf events (uncore/power/cstate): {e:#}; continuing without them"
                    ),
                }
            }
        }

        // Listen to start consumer events, starting sources.
        alumet_event_bus::start_consumer_measurement().subscribe(move |e| {
            for consumer in e.0 {
                let config = config_cloned.lock().unwrap();
                let events = events_of(&config, false);
                let (source, source_name): (Box<dyn Source>, String) = match &consumer {
                    ResourceConsumer::Process { pid } => {
                        // The main thread of the process, whose tid is the pid.
                        let observed = Observed::Task {
                            tid: *pid,
                            inherit: false,
                        };
                        log::info!("Starting to observe {observed:?}...");
                        let source =
                            PerfEventSource::new(&observed, &events, config.multiplexing_auto_scale, consumer.clone())?;
                        (Box::new(source), process_source_name(*pid))
                    }
                    ResourceConsumer::ControlGroup { path } => {
                        // making an assumption about the cgroup mounting point here to be /sys/fs/cgroup
                        // we just have information about the canonical path here
                        // making it hard to not recompute the mounting path here
                        // note that it will only work for cgroup v2
                        // todo: make it dynamic or configurable
                        let absolute_path = format!("/sys/fs/cgroup{path}");
                        if let Some(scan_interval) = config.cgroup_via_processes {
                            // Workaround for the kernels that miscount multiplexed cgroup events.
                            match CgroupProcessesSource::new(
                                PathBuf::from(&absolute_path),
                                &events,
                                config.multiplexing_auto_scale,
                                consumer.clone(),
                                scan_interval,
                            ) {
                                Ok(source) => (Box::new(source), cgroup_source_name(path.clone())),
                                Err(e) => {
                                    log::warn!("cannot observe cgroup {absolute_path}: {e:#}");
                                    continue;
                                }
                            }
                        } else {
                            let Ok(fd) = File::open(&absolute_path) else {
                                panic!("cgroup not found in filesystem: {absolute_path}")
                            };
                            let observed = Observed::Cgroup { fd: Arc::new(fd) };
                            log::info!("Starting to observe cgroup {path}...");
                            let source = PerfEventSource::new(
                                &observed,
                                &events,
                                config.multiplexing_auto_scale,
                                consumer.clone(),
                            )?;
                            (Box::new(source), cgroup_source_name(path.clone()))
                        }
                    }
                    _ => continue,
                };
                let trigger = TriggerSpec::builder(config.poll_interval)
                    .flush_interval(config.flush_interval)
                    .build()?;
                let init_source_state = match config.add_source_in_pause_state {
                    false => TaskState::Run,
                    true => TaskState::Pause,
                };
                drop(config);

                let request =
                    request::create_one().add_source_with_state(&source_name, source, trigger, init_source_state);
                runtime_start.block_on(pipeline_control_start.dispatch(request, Duration::from_secs(1)))?;
                log::debug!("New source {source_name} has started.");
            }
            Ok(())
        });

        // Listen to end consumer events, stopping sources.
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
                runtime_end.block_on(pipeline_control_end.dispatch(stop_request, Duration::from_secs(1)))?;
                log::debug!("Source {source_name} has stopped.");
            }
            Ok(())
        });

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The events of the config counted by the machine source (`system_wide`) or by the process and
/// cgroup sources, each paired with its metric.
fn events_of(config: &ParsedConfig, system_wide: bool) -> Vec<(GroupedEvent, TypedMetricId<u64>)> {
    // System-wide events (uncore, power, cstate…) are not tied to a process or a cgroup: they are
    // counted once, by the machine source.
    config
        .events
        .iter()
        .filter(|(e, _)| matches!(e.event.scope(), event::Scope::SystemWide { .. }) == system_wide)
        .cloned()
        .collect()
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
