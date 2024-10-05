//! Options (CLI and TOML config) for all agents.

use alumet::plugin::PluginMetadata;

pub trait Configurator {
    #[allow(unused_variables)]
    fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {}

    #[allow(unused_variables)]
    fn configure_agent(&mut self, agent: &mut alumet::agent::Builder) {}
}

pub trait ContextDefault {
    fn default_with_context(plugins: &[PluginMetadata]) -> Self;
}

impl<T: Default> ContextDefault for T {
    fn default_with_context(_plugins: &[PluginMetadata]) -> Self {
        T::default()
    }
}

/// Common options for the command-line interface (CLI).
///
/// We use `clap` to parse these options, therefore the structs
/// derive [`clap::Args`].
pub mod cli {
    use clap::Args;
    use std::time::Duration;

    use super::Configurator;

    /// Common CLI arguments.
    ///
    /// # Example and tip
    /// Use `#[command(flatten)]` to add these arguments to your args structure.
    ///
    /// See below:
    ///
    /// ```
    /// use clap::Parser;
    /// use app_agent::options::cli::CommonArgs;
    ///
    /// #[derive(Parser)]
    /// struct Cli {
    ///     #[command(flatten)]
    ///     common: CommonArgs,
    ///
    ///     my_arg: String,
    /// }
    /// ```
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

    impl Configurator for CommonArgs {
        /// Applies the common CLI args to the pipeline.
        fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
            if let Some(max_update_interval) = self.max_update_interval {
                pipeline.trigger_constraints_mut().max_update_interval = max_update_interval;
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
/// We use `serde` to parse these options from the TOML config file,
/// and to write the default configuration to the TOML config file,
/// therefore the structs derive [`serde::Deserialize`] and [`serde::Serialize`].
pub mod config {
    use serde::{Deserialize, Serialize};
    use std::time::Duration;

    use super::Configurator;

    /// Common config options.
    ///
    /// # Example and tip
    /// Use `#[serde(flatten)]` to add these options to your config structure.
    ///
    /// See below:
    ///
    /// ```
    /// use serde::{Deserialize, Serialize};
    /// use app_agent::options::config::CommonOpts;
    ///
    /// #[derive(Deserialize, Serialize)]
    /// struct AgentConfig {
    ///     #[serde(flatten)]
    ///     common: CommonOpts,
    ///
    ///     my_option: String,
    /// }
    /// ```
    #[derive(Deserialize, Serialize)]
    pub struct CommonOpts {
        #[serde(with = "humantime_serde")]
        max_update_interval: Duration,
    }

    impl Configurator for CommonOpts {
        fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
            pipeline.trigger_constraints_mut().max_update_interval = self.max_update_interval;
        }
    }

    impl Default for CommonOpts {
        fn default() -> Self {
            Self {
                max_update_interval: Duration::from_millis(500),
            }
        }
    }
}
