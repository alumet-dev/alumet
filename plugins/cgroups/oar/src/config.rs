use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub(crate) oar_version: OarVersion,
    #[serde(with = "humantime_serde")]
    pub(crate) poll_interval: Duration,
    pub(crate) jobs_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oar_version: OarVersion::Oar3,
            poll_interval: Duration::from_secs(1),
            jobs_only: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OarVersion {
    Oar2,
    Oar3,
}
