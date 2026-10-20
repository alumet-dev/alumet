use alumet::plugin::{
    AlumetPluginStart, AlumetPostStart, ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;

use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
    metrics::Metrics,
    regex::RegexAttributesExtrator,
};

use crate::{
    attr::{JobCgroupRegexExtractorStrategy, SlurmJobTagger},
    config::Config,
};

mod attr;
pub mod config;
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

        // Configure how we detect job metadata in cgroups.
        let tagger_strategy = match &config.job_cgroup_regex {
            config::JobCgroupRegex::SlurmAuto => JobCgroupRegexExtractorStrategy::SlurmAuto {
                v1: RegexAttributesExtrator::new(attr::JOB_REGEX_SLURM1)?,
                v2: RegexAttributesExtrator::new(attr::JOB_REGEX_SLURM2)?,
            },
            config::JobCgroupRegex::SlurmCgroupV1 => {
                JobCgroupRegexExtractorStrategy::Fixed(RegexAttributesExtrator::new(attr::JOB_REGEX_SLURM1)?)
            }
            config::JobCgroupRegex::SlurmCgroupV2 => {
                JobCgroupRegexExtractorStrategy::Fixed(RegexAttributesExtrator::new(attr::JOB_REGEX_SLURM2)?)
            }
            config::JobCgroupRegex::PsSlurm => {
                JobCgroupRegexExtractorStrategy::Fixed(RegexAttributesExtrator::new(attr::JOB_REGEX_PSSLURM)?)
            }
            config::JobCgroupRegex::Custom(regex) => {
                JobCgroupRegexExtractorStrategy::Fixed(RegexAttributesExtrator::new(regex)?)
            }
        };
        let tagger = SlurmJobTagger::new(tagger_strategy);

        let mut shared_hierarchy = OptionalSharedHierarchy::default();

        // If enabled, create the annotation transform.
        if config.annotate_foreign_measurements {
            let shared = SharedCgroupHierarchy::default();
            shared_hierarchy.enable(shared.clone());

            let transform = JobAnnotationTransform {
                tagger: tagger.clone(),
                cgroup_v2_hierarchy: CachedCgroupHierarchy::new(shared),
            };
            alumet.add_transform("slurm/annotation", Box::new(transform))?;
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

pub struct StartingState {
    metrics: Metrics,
    reactor_config: ReactorConfig,
    source_setup: source::JobSourceSetup,
    shared_hierarchy: OptionalSharedHierarchy,
}
