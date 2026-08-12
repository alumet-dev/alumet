use std::{
    fs::File,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    metrics::TypedMetricId,
    pipeline::{control::request, elements::source::trigger::TriggerSpec},
    plugin::{
        AlumetPostStart, event,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use anyhow::Context;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::source::{Observable, PerfEventSourceBuilder};

#[cfg(not(target_os = "linux"))]
compile_error!("This plugin only works on Linux.");

mod cpu;
mod native;
mod pfm;
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
        let config = ParsedConfig {
            // Store the source settings.
            poll_interval: config.poll_interval,
            flush_interval: config.flush_interval,
            // Parse the perf events with the unified syntax.
            events: config
                .events
                .iter()
                .map(spec::parse)
                .try_collect()
                .context("invalid event in config")?,
            // The metrics are initialized in start()
            metrics: Vec::new(),
        };
        Ok(Box::new(PerfPlugin {
            config: Arc::new(Mutex::new(config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let mut config = self.config.lock().unwrap();

        let mut metrics = Vec::with_capacity(config.events.len());
        for e in &config.events {
            let metric_name = format!("perf_{}", e.metric_suffix);
            let metric = alumet.create_metric::<u64>(metric_name, Unit::Unity, e.description.clone())?;
            metrics.push(metric);
        }
        config.metrics = metrics;
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let config_cloned = self.config.clone();
        let pipeline_control = alumet.pipeline_control();
        let runtime = alumet.async_runtime().clone();

        // Listen to events.
        event::start_consumer_measurement().subscribe(move |e| {
            for consumer in e.0 {
                let observable = match consumer {
                    alumet::resources::ResourceConsumer::Process { pid } => Some((
                        Observable::Process {
                            pid: i32::try_from(pid).unwrap(),
                        },
                        format!("source-pid[{pid}]"),
                    )),
                    alumet::resources::ResourceConsumer::ControlGroup { path } => Some((
                        Observable::Cgroup {
                            path: path.to_string(),
                            fd: File::open(path.as_ref()).unwrap(),
                        },
                        format!("source-cgroup[{path}]"),
                    )),
                    _ => None,
                };

                if let Some((o, source_name)) = observable {
                    log::info!("Starting to observe {o:?}...");
                    let config = config_cloned.lock().unwrap();
                    let mut builder = PerfEventSourceBuilder::observe(o)?;
                    for (event, metric) in config.events.iter().zip(&config.metrics) {
                        builder
                            .add(&event.event, *metric)
                            .with_context(|| format!("could not configure event {}", event.metric_suffix))?;
                    }
                    let poll_interval = config.poll_interval;
                    let flush_interval = config.flush_interval;
                    drop(config);

                    let source = builder.build()?;
                    let trigger = TriggerSpec::builder(poll_interval)
                        .flush_interval(flush_interval)
                        .build()?;

                    let request = request::create_one().add_source(&source_name, Box::new(source), trigger);
                    runtime.block_on(pipeline_control.dispatch(request, Duration::from_secs(1)))?;
                    log::debug!("New source has started.");
                }
            }
            Ok(())
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
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
        }
    }
}

// TODO proper deserialization with serde?
struct ParsedConfig {
    poll_interval: Duration,
    flush_interval: Duration,

    events: Vec<spec::ParsedEvent>,
    metrics: Vec<TypedMetricId<u64>>,
}
