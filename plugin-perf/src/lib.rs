use std::{
    fs::File,
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    metrics::TypedMetricId,
    pipeline::trigger::TriggerSpec,
    plugin::{
        event,
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPostStart,
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
        };
        Ok(Box::new(PerfPlugin {
            config: Arc::new(Mutex::new(config)),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
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
        let pipeline_control = alumet.pipeline_control();

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
                    drop(config);

                    let source = builder.build()?;
                    let trigger = TriggerSpec::builder(poll_interval)
                        .flush_interval(flush_interval)
                        .build()?;

                    pipeline_control.add_source(&source_name, Box::new(source), trigger)?;
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

    hardware_events: Vec<String>,
    software_events: Vec<String>,
    cache_events: Vec<String>,
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
}
