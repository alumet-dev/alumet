use std::{
    fs::{self, File}, io::{Read, Seek}, mem, path::PathBuf, sync::Arc, time::Duration
};
use alumet::{
    metrics::TypedMetricId, 
    pipeline::{runtime::ControlHandle, trigger::TriggerSpec, Source, PollError}, 
    plugin::{rust::{deserialize_config, AlumetPlugin, serialize_config}, AlumetStart, ConfigTable, Plugin}, 
    resources::{Resource, ResourceConsumer},
    measurement::{MeasurementAccumulator, Timestamp, MeasurementPoint}, 
    units::{PrefixedUnit, Unit}};
use notify::{recommended_watcher, Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use anyhow::Context;
use log::trace;

#[derive(Debug)]
pub struct Oar2Plugin {
    config: Config,
    metrics: Option<Metrics>,
}
#[derive(Debug, Clone)]
pub struct Metrics {
    cpu_metric: TypedMetricId<u64>,
    memory_metric: TypedMetricId<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    path: PathBuf,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

#[derive(Debug)]
struct OarJobSource{
    cpu_metric: TypedMetricId<u64>,
    memory_metric: TypedMetricId<u64>,
    cgroup_cpu_file: File,
    cgroup_memory_file: File,
}

impl AlumetPlugin for Oar2Plugin {
    fn name() -> &'static str {
        "oar2-plugin"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(Oar2Plugin { config, metrics : None }))
    }

    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), anyhow::Error> {

        // When creating the metric, should the metric id include the job id? -> no
        // Should be stored in the plugin for use in post_pipeline_start.

        let cpu_metric = alumet.create_metric::<u64>(
            "cpu_time",
            PrefixedUnit::nano(Unit::Second),
            "Total CPU time consumed by the cgroup (in nanoseconds).",
        )?;
        let memory_metric = alumet.create_metric::<u64>(
            "memory_usage",
            Unit::Unity,
            "Total memory usage by the cgroup (in bytes).",
        )?;

        self.metrics = Some(Metrics { cpu_metric, memory_metric });

        let cgroup_cpu_path = self.config.path.join("cpuacct/oar");
        let cgroup_memory_path = self.config.path.join("memory/oar");

        // Scanning to check if there are jobs already running
        for entry in fs::read_dir(&cgroup_cpu_path).with_context(|| format!("cgroup cpu path is, my_var={cgroup_cpu_path:?}"))? {
            let entry = entry?;
            let job_name = entry.file_name();
            if entry.file_type()?.is_dir() && job_name.to_string_lossy().chars().any(|c| c.is_numeric()) {
                let cpu_job_path = cgroup_cpu_path.join(&job_name);
                let memory_job_path = cgroup_memory_path.join(&job_name);

                let cpu_file_path = cpu_job_path.join("cpuacct.usage");
                let memory_file_path = memory_job_path.join("memory.usage_in_bytes");

                let cgroup_cpu_file = File::open(&cpu_file_path)
                    .with_context(|| format!("Failed to open CPU usage file at {}", cpu_file_path.display()))?;
                let cgroup_memory_file = File::open(&memory_file_path)
                    .with_context(|| format!("Failed to open memory usage file at {}", memory_file_path.display()))?;

                let initial_source = Box::new(OarJobSource {
                    cpu_metric,
                    memory_metric,
                    cgroup_cpu_file,
                    cgroup_memory_file,
                });

                alumet.add_source(initial_source, TriggerSpec::at_interval(self.config.poll_interval));
            }
        }
        Ok(())
    }

    fn post_pipeline_start(&mut self, pipeline: &mut alumet::pipeline::runtime::RunningPipeline) -> anyhow::Result<()> {
        let control_handle = pipeline.control_handle();
        let config_path = self.config.path.clone();
        let plugin_name = self.name().to_owned();

        let metrics = self.metrics.clone().unwrap();
        let cpu_metric = metrics.cpu_metric;
        let memory_metric = metrics.memory_metric;
        let poll_interval = self.config.poll_interval;

        struct JobDetector{
            config_path: PathBuf,
            cpu_metric: TypedMetricId<u64>,
            memory_metric: TypedMetricId<u64>,
            control_handle: ControlHandle,
            plugin_name: String,
            poll_interval: Duration,
        }

        impl EventHandler for JobDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
                if let Ok(Event { kind: EventKind::Create(_), paths, .. }) = event {
                    for path in paths {
                        if let Some(job_name) = path.file_name() {
                            if job_name.to_string_lossy().chars().any(|c| c.is_numeric()) {
                                let cpu_path = self.config_path.join("cpuacct/oar").join(&job_name);
                                let memory_path = self.config_path.join("memory/oar").join(&job_name);
                                let cpu_file_path = cpu_path.join("cpuacct.usage");
                                let memory_file_path = memory_path.join("memory.usage_in_bytes");
            
                                if let (Ok(cgroup_cpu_file), Ok(cgroup_memory_file)) = (File::open(&cpu_file_path), File::open(&memory_file_path)) {
                                    
                                    let new_source = Box::new(OarJobSource {
                                        cpu_metric: self.cpu_metric,
                                        memory_metric: self.memory_metric,
                                        cgroup_cpu_file,
                                        cgroup_memory_file,
                                    });
        
                                    let source_name = job_name.to_string_lossy().to_string();
            
                                    self.control_handle.add_source(self.plugin_name.clone(), source_name, new_source, TriggerSpec::at_interval(self.poll_interval));
                                }
                            }
                        }
                    }
                } else if let Err(e) = event {
                    eprintln!("watch error: {:?}", e);
                }
            }
        }
            
        let handler = JobDetector { config_path: config_path.clone(), cpu_metric, memory_metric, control_handle, plugin_name, poll_interval };
        let mut watcher = notify::recommended_watcher(handler)?;
            
        watcher.watch(&config_path.join("cpuacct/oar"), RecursiveMode::NonRecursive)?;
        watcher.watch(&config_path.join("memory/oar"), RecursiveMode::NonRecursive)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut path = PathBuf::new();
        path.push("/Users/sofiaduquegomez/Desktop/UGA-Ensimag/M1MoSig/Second Semester/Research Internship/testing");
        Self {
            path,
            poll_interval: Duration::from_secs(1),
        }
    }
}

impl Source for OarJobSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        log::debug!("OarJobSource::poll at {timestamp:?}");
        let cpu_usage_file = &mut self.cgroup_cpu_file;
        cpu_usage_file.rewind()?;
        let mut cpu_usage = String::new();
        cpu_usage_file.read_to_string(&mut cpu_usage)?;
        let memory_usage_file = &mut self.cgroup_memory_file;
        memory_usage_file.rewind()?;
        let mut memory_usage = String::new();
        memory_usage_file.read_to_string(&mut memory_usage)?;
        let cpu_usage_u64 = cpu_usage.trim().parse::<u64>()?;
        let memory_usage_u64 = memory_usage.trim().parse::<u64>()?;

        //Resource consumer: whole path to the job

        measurements.push(MeasurementPoint::new(
            timestamp,
            self.cpu_metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            cpu_usage_u64,
        )/*.with_attr("oar_job_id", the_job_id)*/);

        measurements.push(MeasurementPoint::new(
            timestamp,
            self.memory_metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            memory_usage_u64,
        ));

        Ok(())
    }
}