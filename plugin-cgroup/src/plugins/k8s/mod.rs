use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::rust::{deserialize_config, serialize_config, AlumetPlugin},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
        metrics::Metrics,
    },
    plugins::k8s::{
        pods::{ApiClient, AutoNodePodRegistry},
        token::{Token, TokenRetrievalConfig},
    },
};
use source::SourceSetup;

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
        "cgroups"
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

        // prepare K8S link and test it
        let node = self.config.k8s_node_name();
        let api_token = Token::new(self.config.token_retrieval.clone().into());
        let api_client = ApiClient::new(&self.config.k8s_api_url, api_token)
            .context("failed to create http client for communicating with the K8S API")?;
        let mut pod_registry = AutoNodePodRegistry::new(node, api_client);
        pod_registry
            .refresh()
            .context("failed to list pods with the K8S API, are the url and token correct?")?;

        // store the state for later, because we cannot set up everything now
        let starting_state = StartingState {
            metrics,
            reactor_config,
            pod_registry,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        // continue from the state that has been prepared in `start`
        let s = self.starting_state.take().unwrap();

        let trigger = TriggerSpec::at_interval(self.config.poll_interval);
        let probe_setup = SourceSetup {
            trigger,
            k8s_pods: s.pod_registry,
        };

        let reactor = CgroupReactor::new(
            s.reactor_config,
            s.metrics,
            ReactorCallbacks {
                probe_setup,
                on_removal: NoCallback,
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
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    /// Name of the curent K8S node, defaults to the hostname.
    k8s_node: Option<String>,
    /// URL to the K8S API.
    k8s_api_url: String,
    token_retrieval: TokenRetrievalConfig,

    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            k8s_node: None,
            k8s_api_url: String::from("https://127.0.0.1:8080"),
            token_retrieval: TokenRetrievalConfig::Simple(token::SimpleRetrievalMethod::Auto),
            poll_interval: Duration::from_secs(5),
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
