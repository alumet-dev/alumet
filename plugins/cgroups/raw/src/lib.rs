use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::rust::{deserialize_config, serialize_config, AlumetPlugin},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use util_cgroups_plugins::{
    cgroup_events::{CgroupReactor, NoCallback, ReactorCallbacks, ReactorConfig},
    metrics::Metrics,
};
use source::SourceSetup;

mod source;

/// Gathers metrics from "raw" control groups.
///
/// This plugin is not tied to a particular scheduler or resource manager.
/// It only interacts with the Linux control cgroups, [version 1](https://docs.kernel.org/admin-guide/cgroup-v1/cgroups.html) and/or [version 2](https://docs.kernel.org/admin-guide/cgroup-v2.html), depending on what is available on the system.
pub struct RawCgroupPlugin {
    config: Config,
    starting_state: Option<StartingState>,
    reactor: Option<CgroupReactor>,
}

impl AlumetPlugin for RawCgroupPlugin {
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
        let starting_state = StartingState {
            metrics,
            reactor_config,
        };
        self.starting_state = Some(starting_state);
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        let s = self.starting_state.take().unwrap();
        let probe_setup = SourceSetup {
            trigger: TriggerSpec::at_interval(self.config.poll_interval),
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
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
        }
    }
}
