use std::time::Duration;

use alumet::plugin::{
    AlumetPluginStart, AlumetPostStart, ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        cgroup_events::{CgroupReactor, ReactorCallbacks, ReactorConfig},
        metrics::Metrics,
    },
    plugins::oar::{
        job_tracker::{JobCleaner, JobTracker},
        transform::JobInfoAttacher,
    },
};

mod attr;
mod job_tracker;
mod source;
mod transform;

/// Gathers metrics for OAR jobs.
///
/// Supports OAR2 and OAR3, on cgroup v1 or cgroup v2.
pub struct OarPlugin {
    config: Option<Config>,
    /// Intermediary state for startup.
    starting_state: Option<StartingState>,
    /// The reactor that is running in the background. Dropping it will stop it.
    reactor: Option<CgroupReactor>,
}

impl OarPlugin {
    pub fn new(config: Config) -> Self {
        Self {
            config: Some(config),
            reactor: None,
            starting_state: None,
        }
    }
}

impl AlumetPlugin for OarPlugin {
    fn name() -> &'static str {
        "oar"
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
        let tracker = JobTracker::new();
        let config = self.config.take().unwrap();

        // Prepare for cgroup detection.
        let starting_state = StartingState {
            metrics: Metrics::create(alumet)?,
            reactor_config: ReactorConfig::default(),
            job_cleaner: JobCleaner::with_version(&tracker, config.oar_version)?,
            source_setup: source::JobSourceSetup::new(config, tracker.clone())?,
        };
        self.starting_state = Some(starting_state);

        // Add a transform that adds the list of job ids to every point that does not have the attribute "job_id".
        let transform = JobInfoAttacher::new(tracker);
        alumet.add_transform("oar_job_info_attacher", Box::new(transform))?;
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
                on_removal: Some(s.job_cleaner),
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
    oar_version: OarVersion,
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    jobs_only: bool,
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

struct StartingState {
    metrics: Metrics,
    reactor_config: ReactorConfig,
    source_setup: source::JobSourceSetup,
    job_cleaner: JobCleaner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OarVersion {
    Oar2,
    Oar3,
}
