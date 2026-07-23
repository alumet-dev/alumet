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
use events::NamedPerfEvent;
use itertools::Itertools;
use perf_event::events::{Cache, Hardware, Software};
use serde::{Deserialize, Serialize};

use crate::source::{Observable, PerfEventSourceBuilder};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

mod cpu;
mod events;
mod source;

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
        let config = ParsedConfig {
            // Store the source settings.
            poll_interval: config.poll_interval,
            flush_interval: config.flush_interval,
            // Parse the perf events.
            hardware_events: config
                .hardware_events
                .into_iter()
                .map(|e| events::parse_hardware(&e))
                .try_collect()
                .context("invalid hardware event in config")?,
            software_events: config
                .software_events
                .into_iter()
                .map(|e| events::parse_software(&e))
                .try_collect()
                .context("invalid software event in config")?,
            cache_events: config
                .cache_events
                .into_iter()
                .map(|e| events::parse_cache(&e))
                .try_collect()
                .context("invalid cache event in config")?,
            // The metrics are initialized in start()
            hardware_metrics: Vec::new(),
            software_metrics: Vec::new(),
            cache_metrics: Vec::new(),
            add_source_in_pause_state: config.add_source_in_pause_state,
        };
        Ok(Box::new(PerfPlugin {
            config: Arc::new(Mutex::new(config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        increase_file_descriptors_soft_limit().context("Error while increasing file descriptors soft limit")?;

        let mut config = self.config.lock().unwrap();

        let mut hardware_metrics = Vec::with_capacity(config.hardware_events.len());
        let mut software_metrics = Vec::with_capacity(config.software_events.len());
        let mut cache_metrics = Vec::with_capacity(config.cache_events.len());

        for e in &config.hardware_events {
            let metric_name = format!("perf_hardware_{}", e.name);
            let metric = alumet.create_metric::<u64>(metric_name, Unit::Unity, e.description.clone())?;
            hardware_metrics.push(metric);
        }
        for e in &config.software_events {
            let metric_name = format!("perf_software_{}", e.name);
            let metric = alumet.create_metric::<u64>(metric_name, Unit::Unity, e.description.clone())?;
            software_metrics.push(metric);
        }
        for e in &config.cache_events {
            let metric_name = format!("perf_cache_{}", e.name);
            let metric = alumet.create_metric::<u64>(metric_name, Unit::Unity, e.description.clone())?;
            cache_metrics.push(metric);
        }
        config.hardware_metrics = hardware_metrics;
        config.software_metrics = software_metrics;
        config.cache_metrics = cache_metrics;
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let config_cloned = self.config.clone();
        let pipeline_control_start = alumet.pipeline_control();
        let pipeline_control_end = alumet.pipeline_control();
        let runtime_start = alumet.async_runtime().clone();
        let runtime_end = alumet.async_runtime().clone();

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
                        let fd = File::open(&absolute_path).unwrap();
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
                    let mut builder = PerfEventSourceBuilder::observe(o)?;
                    for (event, metric) in config.hardware_events.iter().zip(&config.hardware_metrics) {
                        builder.add(event.event, *metric).with_context(|| {
                            format!(
                                "could not configure hardware event {} (code {})",
                                event.name, event.event.0
                            )
                        })?;
                    }
                    for (event, metric) in config.software_events.iter().zip(&config.software_metrics) {
                        builder.add(event.event, *metric).with_context(|| {
                            format!(
                                "could not configure software event {} (code {})",
                                event.name, event.event.0
                            )
                        })?;
                    }
                    for (event, metric) in config.cache_events.iter().zip(&config.cache_metrics) {
                        builder
                            .add(event.event.clone(), *metric)
                            .with_context(|| format!("could not configure cache event {}", event.name))?;
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
        .context("Error while setting file descriptors soft limit from {fd_soft} to {fd_hard}")?;
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

    hardware_events: Vec<String>,
    software_events: Vec<String>,
    cache_events: Vec<String>,

    /// If `true`, the perf sources will be started in pause state.
    /// The default value is `false`.
    ///
    /// This behavior is necessary to have fine-grained control over which source to monitor.
    /// !! It's essentially needed for advanced Alumet setup with a control plugin that manage the state of sources.
    #[serde(default)]
    pub add_source_in_pause_state: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),

            hardware_events: vec![
                "REF_CPU_CYCLES".to_owned(),
                "CACHE_MISSES".to_owned(),
                "BRANCH_MISSES".to_owned(),
            ],
            software_events: vec![],
            cache_events: vec!["LL_READ_MISS".to_owned()],

            add_source_in_pause_state: false,
        }
    }
}

// TODO proper deserialization with serde?
struct ParsedConfig {
    poll_interval: Duration,
    flush_interval: Duration,

    hardware_events: Vec<NamedPerfEvent<Hardware>>,
    software_events: Vec<NamedPerfEvent<Software>>,
    cache_events: Vec<NamedPerfEvent<Cache>>,
    hardware_metrics: Vec<TypedMetricId<u64>>,
    software_metrics: Vec<TypedMetricId<u64>>,
    cache_metrics: Vec<TypedMetricId<u64>>,

    add_source_in_pause_state: bool,
}
