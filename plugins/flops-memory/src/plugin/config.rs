use std::{collections::BTreeMap, time::Duration};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    #[serde(with = "humantime_serde")]
    pub(super) poll_interval: Duration,
    #[serde(with = "humantime_serde")]
    pub(super) flush_interval: Duration,
    #[serde(default)]
    pub(super) add_source_in_pause_state: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(5),
            add_source_in_pause_state: false,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct MetricsDocument {
    pub(super) architectures: BTreeMap<String, Vec<ArchitectureRecord>>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArchitectureRecord {
    pub(super) definitions: ArchitectureDefinitions,
    pub(super) metrics: ArchitectureMetrics,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArchitectureDefinitions {
    pub(super) models: Vec<ModelDefinition>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ModelDefinition {
    pub(super) model_code: u64,
    #[serde(default)]
    pub(super) condition: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ArchitectureMetrics {
    #[serde(default)]
    pub(super) flops: Vec<MetricDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct MetricDefinition {
    pub(super) name: String,
    pub(super) units: String,
    pub(super) formula: String,
    pub(super) events: BTreeMap<String, String>,
    #[serde(default)]
    pub(super) unresolved_dependencies: Vec<String>,
}
