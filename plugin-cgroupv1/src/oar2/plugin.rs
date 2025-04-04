use alumet::{
    metrics::TypedMetricId,
    pipeline::{
        control::{request, PluginControlHandle},
        elements::source::trigger::TriggerSpec,
    },
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
    resources::ResourceConsumer,
};
use anyhow::Context;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, time::Duration};

use crate::cgroupv1::Metrics;

use super::{probe::OarJobSource, utils::Cgroupv1MetricFile};

#[derive(Debug)]
pub struct Oar2Plugin {
    config: Config,
    metrics: Option<Metrics>,
    watcher: Option<RecommendedWatcher>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Config {
    path: PathBuf,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl AlumetPlugin for Oar2Plugin {
    fn name() -> &'static str {
        "oar2-plugin"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(Oar2Plugin {
            config,
            metrics: None,
            watcher: None,
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> Result<(), anyhow::Error> {
        let metrics_result = Metrics::new(alumet);
        let metrics = metrics_result?;
        self.metrics = Some(metrics.clone());

        let cgroup_cpu_path = self.config.path.join("cpuacct/oar");
        let cgroup_memory_path = self.config.path.join("memory/oar");

        // Scanning to check if there are jobs already running
        for entry in std::fs::read_dir(&cgroup_cpu_path)
            .with_context(|| format!("Invalid oar cpuacct cgroup path, {cgroup_cpu_path:?}"))?
        {
            let entry = entry?;

            let job_name = entry.file_name();
            let job_name = job_name
                .clone()
                .into_string()
                .ok()
                .with_context(|| format!("Invalid oar username and job id, for job: {:?}", job_name))?;

            if entry.file_type()?.is_dir() && job_name.chars().any(|c| c.is_numeric()) {
                let job_separated = job_name.split_once('_');
                let job_id = job_separated.context("Invalid oar cgroup.")?.1.parse()?;

                let cpu_job_path = cgroup_cpu_path.join(&job_name);
                let memory_job_path = cgroup_memory_path.join(&job_name);

                let cpu_file_path = cpu_job_path.join("cpuacct.usage");
                let memory_file_path = memory_job_path.join("memory.usage_in_bytes");

                let cgroup_cpu_file = File::open(&cpu_file_path)
                    .with_context(|| format!("Failed to open CPU usage file at {}", cpu_file_path.display()))?;
                let cgroup_memory_file = File::open(&memory_file_path)
                    .with_context(|| format!("Failed to open memory usage file at {}", memory_file_path.display()))?;
                let consumer_cpu = ResourceConsumer::ControlGroup {
                    path: cpu_file_path
                        .to_str()
                        .expect("Path to 'cpu.stat' must be valid UTF8")
                        .to_string()
                        .into(),
                };
                let consumer_memory = ResourceConsumer::ControlGroup {
                    path: memory_file_path
                        .to_str()
                        .expect("Path to 'memory.stat' must to be valid UTF8")
                        .to_string()
                        .into(),
                };
                let metric_file = Cgroupv1MetricFile::new(
                    job_id,
                    consumer_cpu,
                    consumer_memory,
                    cgroup_cpu_file,
                    cgroup_memory_file,
                );
                let initial_source = Box::new(OarJobSource {
                    cpu_metric: metrics.cpu_metric,
                    memory_metric: metrics.memory_metric,
                    cgroup_v1_metric_file: metric_file,
                });
                let source_name = &job_name;
                alumet
                    .add_source(
                        source_name,
                        initial_source,
                        TriggerSpec::at_interval(self.config.poll_interval),
                    )
                    .expect("no duplicate job");
            }
        }
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        let config_path = self.config.path.clone();

        let metrics = self.metrics.take().expect("Metrics should be initialized by start()");
        let cpu_metric = metrics.cpu_metric;
        let memory_metric = metrics.memory_metric;
        let poll_interval = self.config.poll_interval;

        struct JobDetector {
            config_path: PathBuf,
            cpu_metric: TypedMetricId<u64>,
            memory_metric: TypedMetricId<u64>,
            control_handle: PluginControlHandle,
            poll_interval: Duration,
            rt: tokio::runtime::Runtime,
        }

        impl EventHandler for JobDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
                fn handle_event_on_path(job_detect: &mut JobDetector, path: PathBuf) -> anyhow::Result<()> {
                    if let Some(job_name) = path.file_name() {
                        let job_name = job_name.to_str().expect("Can't retrieve the job name value");

                        if job_name.chars().any(|c| c.is_numeric()) {
                            let job_separated = job_name.split_once('_');
                            let job_id = job_separated.context("Invalid oar cgroup")?.1.parse()?;

                            let cpu_path = job_detect.config_path.join("cpuacct/oar").join(job_name);
                            log::debug!("CPU path {cpu_path:?}");
                            let memory_path = job_detect.config_path.join("memory/oar").join(job_name);
                            log::debug!("Memory path {memory_path:?}");

                            let cpu_file_path = cpu_path.join("cpuacct.usage");
                            log::debug!("CPU file path {cpu_file_path:?}");
                            let file_cpu = File::open(&cpu_file_path)
                                .with_context(|| format!("failed to open file {}", cpu_file_path.display()))?;
                            let memory_file_path = memory_path.join("memory.usage_in_bytes");
                            log::debug!("Memory file path {memory_file_path:?}");
                            let file_memory = File::open(&memory_file_path)
                                .with_context(|| format!("failed to open file {}", memory_file_path.display()))?;

                            let consumer_cpu = ResourceConsumer::ControlGroup {
                                path: cpu_file_path
                                    .to_str()
                                    .expect("Path to 'cpu.stat' must be valid UTF8")
                                    .to_string()
                                    .into(),
                            };
                            let consumer_memory = ResourceConsumer::ControlGroup {
                                path: memory_file_path
                                    .to_str()
                                    .expect("Path to 'memory.stat' must to be valid UTF8")
                                    .to_string()
                                    .into(),
                            };
                            let metric_file =
                                Cgroupv1MetricFile::new(job_id, consumer_cpu, consumer_memory, file_cpu, file_memory);

                            if let (Ok(_cgroup_cpu_file), Ok(_cgroup_memory_file)) =
                                (File::open(&cpu_file_path), File::open(&memory_file_path))
                            {
                                let new_source = Box::new(OarJobSource {
                                    cpu_metric: job_detect.cpu_metric,
                                    memory_metric: job_detect.memory_metric,
                                    cgroup_v1_metric_file: metric_file,
                                });

                                let source_name = job_name;
                                let trigger = TriggerSpec::at_interval(job_detect.poll_interval);
                                let create_source = request::create_one().add_source(source_name, new_source, trigger);

                                job_detect
                                    .rt
                                    .block_on(
                                        job_detect
                                            .control_handle
                                            .dispatch(create_source, Duration::from_millis(500)),
                                    )
                                    .with_context(|| format!("failed to add source {source_name}"))?;
                            }
                        }
                    }
                    Ok(())
                }

                log::debug!("Handle event function");
                if let Ok(Event {
                    kind: EventKind::Create(_),
                    paths,
                    ..
                }) = event
                {
                    log::debug!("Paths: {paths:?}");
                    for path in paths {
                        if let Err(e) = handle_event_on_path(self, path.clone()) {
                            log::error!("Unable to handle event on {}: {}", path.display(), e);
                        }
                    }
                } else if let Err(e) = event {
                    log::error!("watch error: {:?}", e);
                }
            }
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("tokio Runtime should build")?;

        let handler = JobDetector {
            config_path: config_path.clone(),
            cpu_metric,
            memory_metric,
            control_handle,
            poll_interval,
            rt,
        };
        let mut watcher = notify::recommended_watcher(handler)?;

        watcher.watch(&config_path.join("cpuacct/oar"), RecursiveMode::NonRecursive)?;
        watcher.watch(&config_path.join("memory/oar"), RecursiveMode::NonRecursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut path = PathBuf::new();
        path.push("/sys/fs/cgroup");
        Self {
            path,
            poll_interval: Duration::from_secs(1),
        }
    }
}
