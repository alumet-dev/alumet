use std::time::Duration;

use alumet::plugin::{
    AlumetPluginStart, AlumetPostStart, ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::common::{
        cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
        metrics::Metrics,
    };

mod attr;
mod source;

/// Gathers metrics for slurm jobs.
///
/// Supports slurm on cgroup v1 or cgroup v2.
pub struct SlurmPlugin {
    config: Option<Config>,
    /// Intermediary state for startup.
    starting_state: Option<StartingState>,
    /// The reactor that is running in the background. Dropping it will stop it.
    reactor: Option<CgroupReactor>,
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

        // Prepare for cgroup detection.
        let starting_state = StartingState {
            metrics: Metrics::create(alumet)?,
            reactor_config: ReactorConfig::default(),
            // job_cleaner: JobCleaner::with_version(&tracker, config.oar_version)?,
            source_setup: source::JobSourceSetup::new(config)?,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        // TODO(core) perhaps we could make the control handle available sooner, but return an error if called before the pipeline is ready?
        let s = self.starting_state.take().unwrap();
        let reactor = CgroupReactor::new(
            s.reactor_config,
            s.metrics,
            ReactorCallbacks {
                probe_setup: s.source_setup,
                on_removal: NoCallback,
            },
            alumet.pipeline_control(),
        )
        .context("failed to init CgroupProbeCreator")?;
        self.reactor = Some(reactor);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        drop(self.reactor.take().unwrap());
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    oar_version: SlurmCgroupVersion,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    jobs_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oar_version: SlurmCgroupVersion::V2,
            poll_interval: Duration::from_secs(1),
            jobs_only: true,
        }
    }
}

struct StartingState {
    metrics: Metrics,
    reactor_config: ReactorConfig,
    source_setup: source::JobSourceSetup,
    // job_cleaner: JobCleaner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlurmCgroupVersion {
    V1,
    V2,
}
