use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;

use crate::containers::{AutoContainerRegistry, Config};
use crate::source::SourceSetup;
use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
    metrics::Metrics,
};

mod containers;
mod source;

/// OCI Container runtimes <https://github.com/opencontainers/runtime-spec> plugin: Docker and Podman for now.
pub struct ContainerPlugin {
    config: Config,
    starting_state: Option<StartingState>,
    reactor: Option<CgroupReactor>,
}

impl AlumetPlugin for ContainerPlugin {
    fn name() -> &'static str {
        "containers"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(Self {
            config,
            starting_state: None,
            reactor: None,
        }))
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let metrics = Metrics::create(alumet)?;
        let reactor_config = ReactorConfig::default();
        let mut shared_hierarchy = OptionalSharedHierarchy::default();

        // Prepare OCI container API client and test it
        let api_client = crate::containers::ApiClient::new().context("failed to create API client")?;

        let mut container_registry = AutoContainerRegistry::new(api_client.clone());
        container_registry
            .refresh()
            .context("failed to refresh containers registry?")?;

        log::debug!(
            "Successfully connected to runtime API and loaded {} containers",
            container_registry.containers.len()
        );

        // If enabled, create the annotation transform to annotate measurements from other plugins
        if self.config.annotate_foreign_measurements {
            let shared = SharedCgroupHierarchy::default();
            shared_hierarchy.enable(shared.clone());

            let transform = JobAnnotationTransform {
                tagger: container_registry.clone(),
                cgroup_v2_hierarchy: CachedCgroupHierarchy::new(shared),
            };
            alumet.add_transform("containers-annotation", Box::new(transform))?;
        }

        // Store the state for later use in post_pipeline_start
        let starting_state = StartingState {
            metrics,
            reactor_config,
            container_registry,
            opt_shared_hierarchy: shared_hierarchy,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        // Continue from the state prepared in `start`
        let s = self.starting_state.take().unwrap();

        let trigger = TriggerSpec::at_interval(self.config.poll_interval);

        let source_setup = SourceSetup {
            trigger,
            container_registry: s.container_registry,
        };

        let reactor = CgroupReactor::new(
            s.reactor_config,
            s.metrics,
            ReactorCallbacks {
                probe_setup: source_setup,
                on_removal: NoCallback,
                on_fs_mount: s.opt_shared_hierarchy,
            },
            alumet.pipeline_control(),
        )
        .context("failed to init CgroupReactor")?;

        self.reactor = Some(reactor);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        drop(self.reactor.take().unwrap());
        Ok(())
    }
}

struct StartingState {
    metrics: Metrics,
    reactor_config: ReactorConfig,
    container_registry: AutoContainerRegistry,
    opt_shared_hierarchy: OptionalSharedHierarchy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_name() {
        assert_eq!(ContainerPlugin::name(), "containers");
    }

    #[test]
    fn test_plugin_version() {
        let version = ContainerPlugin::version();
        assert!(!version.is_empty());
        assert!(version.contains('.'));
    }

    #[test]
    fn test_default_config() {
        let config_table = Config::default();
        assert_eq!(config_table.poll_interval, std::time::Duration::from_secs(5));
        assert!(!config_table.annotate_foreign_measurements);
    }
}
