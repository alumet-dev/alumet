use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config::{Config, ContainerRuntime};
use crate::registry::ContainerRegistry;
use crate::source::SourceSetup;
use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
    metrics::Metrics,
};

mod client;
mod config;
mod extraction;
mod registry;
mod source;

/// Container plugin for Docker and Podman.
///
/// This plugin provides annotations for cgroup measurements based on container metadata
/// from Docker or Podman APIs. It can annotate both its own measurements and measurements
/// from other plugins.
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

        // Prepare Docker/Podman API client and test it
        let runtime = self.config.runtime;
        
        // Determine the API URL to use
        let api_url = {
            if let Some(ref socket_path) = self.config.socket_path {
                // Use explicit socket path if provided
                log::info!("Using explicit socket path: {}", socket_path);
                format!("unix://{}", socket_path)
            } else if self.config.detect_wsl {
                // Try to detect WSL2 environment and use appropriate socket
                if let Some(wsl_socket) = crate::client::detect_wsl_socket_path(runtime) {
                    log::info!("Using detected WSL2 socket: {}", wsl_socket);
                    wsl_socket
                } else {
                    // Fallback to default URL
                    self.config.api_url()
                }
            } else {
                // Use configured URL or default
                self.config.api_url()
            }
        };
        
        let api_client = ApiClient::new(runtime, &api_url)
            .with_context(|| {
                format!("failed to create API client for {} at {}. If running in WSL2, try setting api_url to 'unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock' or specify socket_path in configuration.", 
                        runtime, api_url)
            })?;
        
        let mut container_registry = ContainerRegistry::new(api_client.clone(), self.config.clone());
        container_registry.refresh()
            .with_context(|| {
                format!("failed to list containers with {} API, is the URL '{}' correct?", runtime, api_url)
            })?;
        
        log::info!("Successfully connected to {} API and loaded {} containers", 
                   runtime, container_registry.containers.len());

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
    container_registry: ContainerRegistry,
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
        assert_eq!(config_table.runtime, ContainerRuntime::Docker);
        assert_eq!(config_table.poll_interval, Duration::from_secs(5));
        assert!(!config_table.annotate_foreign_measurements);
        assert!(!config_table.include_container_labels);
    }
}