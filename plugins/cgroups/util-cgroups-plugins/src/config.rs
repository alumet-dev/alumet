use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct CommonConfig {
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    pub disable_sources: bool,
}

impl Default for CommonConfig {
    #[cfg_attr(tarpaulin, ignore)]
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            disable_sources: false,
        }
    }
}
