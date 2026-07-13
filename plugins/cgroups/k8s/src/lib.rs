use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    pods::{ApiClient, AutoNodePodRegistry},
    token::{Token, TokenRetrievalConfig},
};
use source::SourceSetup;
use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    config::CommonConfig,
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
    metrics::Metrics,
};

mod pods;
mod source;
mod token;

pub struct K8sPlugin {
    config: Config,
    starting_state: Option<StartingState>,
    reactor: Option<CgroupReactor>,
}

impl AlumetPlugin for K8sPlugin {
    fn name() -> &'static str {
        "k8s"
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
        let annotate_containers = self.config.annotate_containers;

        // prepare K8S link and test it
        let node = self.config.k8s_node_name();
        let api_token = Token::new(self.config.token_retrieval.clone().into());
        let api_client = ApiClient::new(&self.config.k8s_api_url, api_token)
            .context("failed to create http client for communicating with the K8S API")?;
        let mut pod_registry = AutoNodePodRegistry::new(node, api_client, annotate_containers);
        pod_registry
            .refresh()
            .context("failed to list pods with the K8S API, are the url and token correct?")?;
        log::info!("List of pods refreshed.");

        // If enabled, create the annotation transform.
        if self.config.annotate_foreign_measurements {
            let shared = SharedCgroupHierarchy::default();
            shared_hierarchy.enable(shared.clone());

            let transform = JobAnnotationTransform {
                tagger: pod_registry.clone(),
                cgroup_v2_hierarchy: CachedCgroupHierarchy::new(shared),
            };
            alumet.add_transform("k8s-annotation", Box::new(transform))?;
        }

        // store the state for later, because we cannot set up everything now
        let starting_state = StartingState {
            metrics,
            reactor_config,
            pod_registry,
            opt_shared_hierarchy: shared_hierarchy,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        // continue from the state that has been prepared in `start`
        let s = self.starting_state.take().unwrap();

        let trigger = TriggerSpec::at_interval(self.config.common.poll_interval);
        let probe_setup = SourceSetup {
            start_sources: !self.config.common.disable_sources,
            trigger,
            k8s_pods: s.pod_registry,
        };

        let reactor = CgroupReactor::new(
            s.reactor_config,
            s.metrics,
            ReactorCallbacks {
                probe_setup,
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
    pod_registry: AutoNodePodRegistry,
    opt_shared_hierarchy: OptionalSharedHierarchy,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub common: CommonConfig,
    /// Name of the curent K8S node, defaults to the hostname.
    pub k8s_node: Option<String>,
    /// URL to the K8S API.
    #[serde(default = "default_k8s_api_url")]
    pub k8s_api_url: String,
    pub token_retrieval: TokenRetrievalConfig,

    /// If `true`, adds attributes like `uid`, `name`, `namespace`, `node` to the cgroup measurements produced by other plugins.
    #[serde(default)]
    pub annotate_foreign_measurements: bool,
    /// Decides whether the cgroups at container level should be annotated or not.
    /// A `false` value will only annotate pod cgroups.
    /// Note that `annotate_foreign_measurements` needs to be true.
    pub annotate_containers: bool,
}

fn default_k8s_api_url() -> String {
    String::from("http://127.0.0.1:8080")
}

impl Default for Config {
    fn default() -> Self {
        Self {
            common: CommonConfig::default(),
            k8s_node: None,
            k8s_api_url: default_k8s_api_url(),
            token_retrieval: TokenRetrievalConfig::Simple(token::SimpleRetrievalMethod::Auto),
            annotate_foreign_measurements: false,
            annotate_containers: false,
        }
    }
}

impl Config {
    fn k8s_node_name(&self) -> String {
        match &self.k8s_node {
            Some(node) => node.clone(),
            None => hostname::get()
                .expect("failed to get the node's hostname")
                .into_string()
                .expect("hostname should be valid utf-8"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_k8s_node_name() {
        let config = Config {
            k8s_node: Some("test-node".into()),
            ..Default::default()
        };

        assert_eq!(config.k8s_node_name(), "test-node");
    }
}
