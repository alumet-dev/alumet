use std::time::Duration;

use alumet::plugin::{
    AlumetPluginStart, AlumetPostStart, ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
    metrics::Metrics,
};

use crate::attr::SlurmJobTagger;

mod attr;
mod source;

/// Gathers metrics for slurm jobs.
///
/// Supports slurm on cgroup v1 or cgroup v2.
pub struct SlurmPlugin {
    pub config: Option<Config>,
    /// Intermediary state for startup.
    pub starting_state: Option<StartingState>,
    /// The reactor that is running in the background. Dropping it will stop it.
    pub reactor: Option<CgroupReactor>,
}

impl SlurmPlugin {
    pub fn new(config: Config) -> Self {
        Self {
            config: Some(config),
            reactor: None,
            starting_state: None,
        }
    }
}

impl AlumetPlugin for SlurmPlugin {
    fn name() -> &'static str {
        "slurm"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(Self::new(config)))
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let config = self.config.take().unwrap();

        let tagger = SlurmJobTagger::new()?;
        let mut shared_hierarchy = OptionalSharedHierarchy::default();

        // If enabled, create the annotation transform.
        if config.annotate_foreign_measurements {
            let shared = SharedCgroupHierarchy::default();
            shared_hierarchy.enable(shared.clone());

            let transform = JobAnnotationTransform {
                tagger: tagger.clone(),
                cgroup_v2_hierarchy: CachedCgroupHierarchy::new(shared),
            };
            alumet.add_transform("slurm-annotation", Box::new(transform))?;
        }

        // Prepare for cgroup detection.
        let starting_state = StartingState {
            metrics: Metrics::create(alumet)?,
            reactor_config: ReactorConfig {
                add_source_in_pause_state: config.add_source_in_pause_state,
                ..Default::default()
            },
            source_setup: source::JobSourceSetup::new(config, tagger)?,
            shared_hierarchy,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let s = self.starting_state.take().unwrap();

        let reactor = CgroupReactor::new(
            s.reactor_config,
            s.metrics,
            ReactorCallbacks {
                probe_setup: s.source_setup,
                on_removal: NoCallback,
                on_fs_mount: s.shared_hierarchy,
            },
            alumet.pipeline_control(),
        )
        .context("failed to init CgroupProbeCreator")?;
        self.reactor = Some(reactor);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if let Some(reactor) = self.reactor.take() {
            drop(reactor);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Interval between two measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Interval between two scans of the cgroup v1 hierarchies.
    /// Only applies to cgroup v1 hierarchies (cgroupv2 supports inotify).
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub cgroupv1_refresh_interval: Option<Duration>,

    /// Only monitor the cgroups related to slurm jobs.
    pub ignore_non_jobs: bool,

    /// At which level do we monitor the Slurm jobs.
    pub jobs_monitoring_level: JobMonitoringLevel,

    /// If `true`, the slurm sources will be started in pause state.
    /// The default value is `false`.
    ///
    /// This behavior is necessary to have fine-grained control over which cgroup to monitor.
    /// !! It's essentially needed for advanced Alumet setup with a control plugin that manage the state of sources.
    #[serde(default)]
    pub add_source_in_pause_state: bool,

    /// If `true`, adds attributes like `job_id` to the measurements produced by other plugins.
    /// The default value is `false`.
    ///
    /// The measurements must have the `cgroup` resource consumer, and **cgroup v2** must be used on the node.
    #[serde(default)]
    pub annotate_foreign_measurements: bool,
}

impl Default for Config {
    #[cfg_attr(tarpaulin, ignore)]
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            cgroupv1_refresh_interval: None,
            ignore_non_jobs: true,
            jobs_monitoring_level: JobMonitoringLevel::Job,
            add_source_in_pause_state: false,
            annotate_foreign_measurements: false,
        }
    }
}

pub struct StartingState {
    metrics: Metrics,
    reactor_config: ReactorConfig,
    source_setup: source::JobSourceSetup,
    shared_hierarchy: OptionalSharedHierarchy,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum JobMonitoringLevel {
    Job,
    Step,
    SubStep,
    Task,
}
