use alumet::{
    pipeline::{
        control::{request, PluginControlHandle},
        elements::source::trigger::TriggerSpec,
    },
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
};
use anyhow::Context;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::cgroupv1::Metrics;

use super::probe::OAR2Probe;

#[derive(Debug)]
pub struct Oar2Plugin {
    metrics: Option<Metrics>,
    watcher: Option<RecommendedWatcher>,
    cpuacct_controller_path: PathBuf,
    memory_controller_path: PathBuf,
    trigger: TriggerSpec,
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

    fn init(config_table: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config_table)?;
        let cpuacct_controller_path = config.path.clone().join("cpuacct/oar");
        let memory_controller_path = config.path.clone().join("memory/oar");
        let poll_interval = config.poll_interval;
        Ok(Box::new(Oar2Plugin {
            cpuacct_controller_path,
            memory_controller_path,
            metrics: None,
            watcher: None,
            trigger: TriggerSpec::at_interval(poll_interval),
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> Result<(), anyhow::Error> {
        let metrics = Metrics::new(alumet)?;
        self.metrics = Some(metrics.clone());

        // Scanning to check if there are jobs already running
        for entry in
            std::fs::read_dir(&self.cpuacct_controller_path).with_context(|| "Invalid oar cpuacct cgroup path")?
        {
            let entry = entry?;

            let job_name = entry.file_name();
            let job_name = job_name
                .clone()
                .into_string()
                .ok()
                .with_context(|| format!("Invalid oar username and job id, for job: {:?}", job_name))?;

            if entry.file_type()?.is_dir() && job_name.chars().any(|c| c.is_numeric()) {
                let source = Oar2Plugin::new_job_source(
                    &job_name,
                    metrics.clone(),
                    &self.cpuacct_controller_path,
                    &self.memory_controller_path,
                )?;

                let source_name = &job_name;
                alumet
                    .add_source(source_name, source, self.trigger.clone())
                    .expect("no duplicate job");
            }
        }
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        let trigger = self.trigger.clone();
        let metrics = self.metrics.as_ref().unwrap().clone();
        let cpuacct_controller_path = self.cpuacct_controller_path.clone();
        let memory_controller_path = self.memory_controller_path.clone();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("tokio Runtime should build")?;

        let handler = JobDetector {
            cpuacct_controller_path: cpuacct_controller_path.clone(),
            memory_controller_path: memory_controller_path.clone(),
            control_handle,
            metrics,
            trigger,
            rt,
        };
        let mut watcher = notify::recommended_watcher(handler)?;

        watcher.watch(&cpuacct_controller_path, RecursiveMode::NonRecursive)?;
        watcher.watch(&memory_controller_path, RecursiveMode::NonRecursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Oar2Plugin {
    fn job_id_from_name(name: &str) -> Result<String, anyhow::Error> {
        Ok(name.split_once('_').context("Invalid oar cgroup")?.1.parse()?)
    }

    //TODO: could implement here some configuration that would enable/disable some metrics collections (using filepaths Options)
    fn new_job_source(
        job_name: &String,
        metrics: Metrics,
        cpuacct_controller_path: &Path,
        memory_controller_path: &Path,
    ) -> Result<Box<OAR2Probe>, anyhow::Error> {
        let job_id = Self::job_id_from_name(job_name)?;

        let cpuacct_controller_job_path = cpuacct_controller_path.join(job_name);
        log::debug!("CPU path {cpuacct_controller_job_path:?}");
        let memory_controller_job_path = memory_controller_path.join(job_name);
        log::debug!("Memory path {memory_controller_job_path:?}");

        let cpuacct_usage_filepath = cpuacct_controller_job_path
            .join("cpuacct.usage")
            .to_str()
            .unwrap()
            .to_string();
        let memory_usage_filepath = memory_controller_job_path
            .join("memory.usage_in_bytes")
            .to_str()
            .unwrap()
            .to_string();

        Ok(Box::new(OAR2Probe::new(
            job_id,
            metrics,
            Some(cpuacct_usage_filepath),
            Some(memory_usage_filepath),
        )?))
    }
}

struct JobDetector {
    cpuacct_controller_path: PathBuf,
    memory_controller_path: PathBuf,
    control_handle: PluginControlHandle,
    metrics: Metrics,
    trigger: TriggerSpec,
    rt: tokio::runtime::Runtime,
}

impl EventHandler for JobDetector {
    fn handle_event(&mut self, event: Result<Event, notify::Error>) {
        fn handle_event_on_path(job_detect: &mut JobDetector, path: PathBuf) -> anyhow::Result<()> {
            if let Some(job_name) = path.file_name() {
                let job_name = job_name.to_str().unwrap().to_string();

                if job_name.chars().any(|c| c.is_numeric()) {
                    let source = Oar2Plugin::new_job_source(
                        &job_name,
                        job_detect.metrics.clone(),
                        &job_detect.cpuacct_controller_path,
                        &job_detect.memory_controller_path,
                    )?;

                    let source_name = job_name;
                    let new_job_source =
                        request::create_one().add_source(&source_name, source, job_detect.trigger.clone());

                    job_detect
                        .rt
                        .block_on(
                            job_detect
                                .control_handle
                                .dispatch(new_job_source, Duration::from_millis(500)),
                        )
                        .with_context(|| format!("failed to add source {source_name}"))?;
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
