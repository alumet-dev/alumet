use rlimit::{Resource, getrlimit, setrlimit};
use std::{
    fs::File,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    metrics::TypedMetricId,
    pipeline::{
        control::{matching::SourceMatcher, request},
        elements::source::{control::TaskState, trigger::TriggerSpec},
        matching::{SourceNamePattern, StringPattern},
    },
    plugin::{
        AlumetPostStart, event,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::source::{Observable, PerfEventSource, PerfEventSourceBuilder};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

mod cpu;
mod multiplexing;
mod native;
mod pfm;
mod pmu;
mod raw;
mod source;
mod spec;

pub struct PerfPlugin {
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
            events.extend(spec::parse(entry).context("invalid event in config")?);
        }
        let config = ParsedConfig {
            // Store the source settings.
            poll_interval: config.poll_interval,
            flush_interval: config.flush_interval,
            events,
            multiplexing_auto_scale: config.multiplexing_auto_scale,
            // The metrics are initialized in start()
            metrics: Vec::new(),
            add_source_in_pause_state: config.add_source_in_pause_state,
        };
        Ok(Box::new(PerfPlugin {
            config: Arc::new(Mutex::new(config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        increase_file_descriptors_soft_limit().context("Error while increasing file descriptors soft limit")?;

        let mut config = self.config.lock().unwrap();

        // Events fanned onto several core PMUs share one metric (same name, told apart by their
        // `pmu` attribute), so a metric is created once per distinct name and reused.
        let mut metrics = Vec::with_capacity(config.events.len());
        let mut by_name: std::collections::HashMap<String, TypedMetricId<u64>> = std::collections::HashMap::new();
        for e in &config.events {
            let metric_name = format!("perf_{}", e.metric_suffix);
            let metric = match by_name.get(&metric_name) {
                Some(metric) => *metric,
                None => {
                    let metric = alumet.create_metric::<u64>(&metric_name, Unit::Unity, e.description.clone())?;
                    by_name.insert(metric_name, metric);
                    metric
                }
            };
            metrics.push(metric);
        }
        config.metrics = metrics;
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
            let system_events: Vec<_> = config
                .events
                .iter()
                .zip(&config.metrics)
                .filter_map(|(event, metric)| match &event.scope {
                    spec::Scope::SystemWide { pmu, cpus } => {
                        Some((event.event.clone(), *metric, pmu.clone(), cpus.clone()))
                    }
                    spec::Scope::TaskAttached { .. } => None,
                })
                .collect();
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

                match PerfEventSource::build_system_wide(auto_scale, system_events) {
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
        event::start_consumer_measurement().subscribe(move |e| {
            for consumer in e.0 {
                let observable = match consumer {
                    alumet::resources::ResourceConsumer::Process { pid } => Some((
                        Observable::Process {
                            pid: i32::try_from(pid).unwrap(),
                        },
                        process_source_name(pid),
                    )),
                    alumet::resources::ResourceConsumer::ControlGroup { path } => {
                        // making an assumption about the cgroup mounting point here to be /sys/fs/cgroup
                        // we just have information about the canonical path here
                        // making it hard to not recompute the mounting path here
                        // note that it will only work for cgroup v2
                        // todo: make it dynamic or configurable
                        let absolute_path = format!("/sys/fs/cgroup{}", path.to_string());
                        let Ok(fd) = File::open(&absolute_path) else {
                            panic!("cgroup not found in filesystem: {absolute_path}")
                        };
                        Some((
                            Observable::Cgroup {
                                path: absolute_path,
                                fd,
                            },
                            cgroup_source_name(path),
                        ))
                    }
                    _ => None,
                };

                if let Some((o, source_name)) = observable {
                    log::info!("Starting to observe {o:?}...");
                    let config = config_cloned.lock().unwrap();
                    let mut builder = PerfEventSourceBuilder::observe(o, config.multiplexing_auto_scale)?;
                    for (event, metric) in config.events.iter().zip(&config.metrics) {
                        match &event.scope {
                            // System-wide events (uncore, power, cstate…) are not tied to a
                            // process/cgroup; they will be opened once by a dedicated machine-wide
                            // source. Skip them here so they don't break this entity source.
                            spec::Scope::SystemWide { .. } => (),
                            spec::Scope::TaskAttached { binding } => {
                                builder
                                    .add(&event.event, *metric, binding.as_ref())
                                    .with_context(|| format!("could not configure event {}", event.metric_suffix))?;
                            }
                        }
                    }
                    let poll_interval = config.poll_interval;
                    let flush_interval = config.flush_interval;
                    let add_source_in_pause_state = config.add_source_in_pause_state;
                    drop(config);

                    let source = builder.build()?;
                    let trigger = TriggerSpec::builder(poll_interval)
                        .flush_interval(flush_interval)
                        .build()?;

                    let init_source_state = match add_source_in_pause_state {
                        false => TaskState::Run,
                        true => TaskState::Pause,
                    };

                    let request = request::create_one().add_source_with_state(
                        &source_name,
                        Box::new(source),
                        trigger,
                        init_source_state,
                    );
                    runtime_start.block_on(pipeline_control_start.dispatch(request, Duration::from_secs(1)))?;
                    log::debug!("New source {source_name} has started.");
                }
            }
            Ok(())
        });

        // Listen to end consumer events, stopping sources.
        event::end_consumer_measurement().subscribe(move |e| {
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,

    /// The events to measure, described with the unified syntax (see [`spec`]).
    ///
    /// Each entry is either a bare string (`"REF_CPU_CYCLES"`, `"INSTRUCTIONS:u"`) or an inline
    /// table with an optional metric `rename` (`{ event = "LL_READ_MISS", rename = "llc_miss" }`).
    events: Vec<spec::EventEntry>,

    /// If `true`, the perf sources will be started in pause state.
    /// The default value is `false`.
    ///
    /// This behavior is necessary to have fine-grained control over which source to monitor.
    /// !! It's essentially needed for advanced Alumet setup with a control plugin that manage the state of sources.
    #[serde(default)]
    pub add_source_in_pause_state: bool,

    /// Whether to compensate for the multiplexing of the perf events.
    ///
    /// A CPU only has a few hardware counters. When more events are requested than it can hold, the
    /// kernel only counts them part of the time, and the raw values are underestimated. When this is
    /// enabled (the default), the plugin extrapolates the missing part, like the `perf` tool does.
    /// When disabled, the raw values are reported as they are.
    ///
    /// Either way, every measurement carries an `accuracy` attribute telling whether its value is
    /// exact, extrapolated or underestimated.
    multiplexing_auto_scale: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),

            events: vec![
                spec::EventEntry::Simple("REF_CPU_CYCLES".to_owned()),
                spec::EventEntry::Simple("CACHE_MISSES".to_owned()),
                spec::EventEntry::Simple("BRANCH_MISSES".to_owned()),
                spec::EventEntry::Simple("LL_READ_MISS".to_owned()),
            ],

            add_source_in_pause_state: false,

            multiplexing_auto_scale: true,
        }
    }
}

// TODO proper deserialization with serde?
struct ParsedConfig {
    poll_interval: Duration,
    flush_interval: Duration,

    events: Vec<spec::ParsedEvent>,
    metrics: Vec<TypedMetricId<u64>>,

    add_source_in_pause_state: bool,

    multiplexing_auto_scale: bool,
}
