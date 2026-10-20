use std::time::Duration;

use serde::{Deserialize, Serialize};
use util_cgroups_plugins::config::CommonConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub common: CommonConfig,

    /// Interval between two scans of the cgroup v1 hierarchies.
    /// Only applies to cgroup v1 hierarchies (cgroupv2 supports inotify).
    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub cgroupv1_refresh_interval: Option<Duration>,

    /// Only monitor the cgroups related to slurm jobs.
    pub ignore_non_jobs: bool,

    /// At which level do we monitor the Slurm jobs?
    pub jobs_monitoring_level: JobMonitoringLevel,

    /// At which level should the plugin emit events destined to other plugins?
    pub jobs_event_level: JobMonitoringLevel,

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

    /// Regex to apply to cgroup paths to extract job metadata.
    ///
    /// The most common options are available as special keys, for example "slurm_auto", which is the default value.
    #[serde(default)]
    pub job_cgroup_regex: JobCgroupRegex,
}

impl Default for Config {
    #[cfg_attr(tarpaulin, ignore)]
    fn default() -> Self {
        Self {
            common: CommonConfig::default(),
            cgroupv1_refresh_interval: None,
            ignore_non_jobs: true,
            jobs_monitoring_level: JobMonitoringLevel::Job,
            jobs_event_level: JobMonitoringLevel::Job,
            add_source_in_pause_state: false,
            annotate_foreign_measurements: false,
            job_cgroup_regex: JobCgroupRegex::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum JobMonitoringLevel {
    Job,
    Step,
    SubStep,
    Task,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobCgroupRegex {
    /// Parses Slurm cgroup paths. Detects cgroup v1 and cgroup v2 automatically.
    #[default]
    SlurmAuto,

    /// Parses Slurm cgroup paths for cgroup v1.
    SlurmCgroupV1,

    /// Parses Slurm cgroup paths for cgroup v2.
    SlurmCgroupV2,

    /// Parses `psslurm` cgroup paths, which are different from the standard Slurm paths.
    #[serde(rename = "psslurm")]
    PsSlurm,

    /// A custom regex.
    #[serde(untagged)]
    Custom(String),
}
