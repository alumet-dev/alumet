use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub(crate) oar_version: OarVersion,
    #[serde(with = "humantime_serde")]
    pub(crate) poll_interval: Duration,
    pub(crate) jobs_only: bool,
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
            oar_version: OarVersion::Oar3,
            poll_interval: Duration::from_secs(1),
            jobs_only: true,
            annotate_foreign_measurements: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OarVersion {
    Oar2,
    Oar3,
}
