//! Options common to all agents.

use alumet::agent::{Agent, AgentConfig};

pub trait AgentModifier {
    fn apply_to(self, agent: &mut Agent, config: &mut AgentConfig);
}

/// Common CLI options.
pub mod cli {
    use alumet::agent::{Agent, AgentConfig};
    use clap::Args;
    use std::time::Duration;

    /// Common CLI arguments.
    ///
    /// Use `#[command(flatten)]` to add these arguments to your args structure.
    #[derive(Args, Clone)]
    pub struct CommonArgs {
        /// Path to the config file.
        #[arg(long, default_value = "alumet-config.toml")]
        pub config: String,
        /// Maximum amount of time between two updates of the sources' commands.
        ///
        /// A lower value means that the latency of source commands will be lower,
        /// i.e. commands will be applied faster, at the cost of a higher overhead.
        #[arg(long, value_parser = humantime_serde::re::humantime::parse_duration)]
        pub max_update_interval: Option<Duration>,
    }

    impl super::AgentModifier for CommonArgs {
        /// Applies the common CLI args to the agent.
        fn apply_to(self, agent: &mut Agent, _: &mut AgentConfig) {
            if let Some(max_update_interval) = self.max_update_interval {
                agent.source_trigger_constraints().max_update_interval = max_update_interval;
            }
        }
    }

    /// CLI arguments for the `exec` command.
    #[derive(Args, Clone)]
    pub struct ExecArgs {
        /// The program to run.
        pub program: String,

        /// Arguments to the program.
        #[arg(trailing_var_arg = true)]
        pub args: Vec<String>,
    }
}

/// Common configuration options (for the app, not the plugins).
///
/// Use `#[serde(flatten)]` to add these options to your options structure.
pub mod config {
    use alumet::agent::{Agent, AgentConfig};
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    #[derive(Deserialize, Serialize)]
    pub struct CommonArgs {
        #[serde(with = "humantime_serde")]
        max_update_interval: Duration,
    }

    impl super::AgentModifier for CommonArgs {
        /// Applies the common config options to the agent.
        fn apply_to(self, agent: &mut Agent, _: &mut AgentConfig) {
            agent.source_trigger_constraints().max_update_interval = self.max_update_interval;
        }
    }

    impl Default for CommonArgs {
        fn default() -> Self {
            Self {
                max_update_interval: Duration::from_millis(500),
            }
        }
    }
}
