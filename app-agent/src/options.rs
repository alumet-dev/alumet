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
    use alumet::plugin::PluginMetadata;
    use anyhow::{anyhow, Context};
    use clap::Args;
    use std::{collections::HashSet, time::Duration};

    use crate::config_ops;

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
    /// use alumet_agent::options::cli::CommonArgs;
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
        #[arg(long, env = "ALUMET_CONFIG", default_value = "alumet-config.toml")]
        pub config: String, // not used in Configurator, but directly by main()

        /// If set, the config file must exist, otherwise the agent will fail to start with an error.
        #[arg(long, default_value_t = false)]
        pub no_default_config: bool,

        /// Config options overrides.
        ///
        /// Use dots to separate TOML levels, ex. `plugins.rapl.poll_interval='1ms'`
        #[arg(long)]
        pub config_override: Option<Vec<String>>,

        /// List of plugins to enable, separated by commas, ex. `csv,rapl`.
        ///
        /// All the other plugins will be disabled.
        #[arg(long, value_delimiter = ',')]
        pub plugins: Option<Vec<String>>,

        /// Maximum amount of time between two updates of the sources' commands.
        ///
        /// A lower value means that the latency of source commands will be lower,
        /// i.e. commands will be applied faster, at the cost of a higher overhead.
        #[arg(long, value_parser = humantime_serde::re::humantime::parse_duration)]
        pub max_update_interval: Option<Duration>,

        /// How many `MeasurementBuffer`s can be stored in the channel that sources write to.
        ///
        /// You may want to increase this if you get "`buffer is full`" errors, which can happen
        /// if you have a large number of sources that flush at the same time.
        #[arg(long)]
        pub source_channel_size: Option<usize>,

        /// How many "normal" worker threads to spawn.
        #[arg(long, env = "ALUMET_NORMAL_THREADS")]
        pub normal_worker_threads: Option<usize>,

        /// How many "high-priority" worker threads to spawn.
        #[arg(long, env = "ALUMET_PRIORITY_THREADS")]
        pub priority_worker_threads: Option<usize>,
    }

    impl CommonArgs {
        pub fn take_config_override_table(
            &mut self,
            plugins: &[PluginMetadata],
        ) -> anyhow::Result<Option<toml::Table>> {
            if self.config_override.is_none() && self.plugins.is_none() {
                // nothing to override
                return Ok(None);
            }

            let mut res = toml::Table::new();

            // apply config overrides
            if let Some(config_override) = self.config_override.take() {
                for o in config_override {
                    let overrider: toml::Table =
                        toml::from_str(&o).with_context(|| format!("invalid config override `{o}`"))?;
                    config_ops::merge_override(&mut res, overrider);
                }
            }

            // apply the list of enabled plugins
            if let Some(enabled_plugins) = self.plugins.take() {
                // Plugins are enabled by default, what we need to override is the "enable"
                // config entry of the *disabled* plugins.
                // Also, the config may disable some plugins, and the args must override that,
                // so we also need to override the "enable" config entry of *enabled* plugins.

                // Find the enabled and disabled plugins
                let mut enabled_set: HashSet<String> = HashSet::from_iter(enabled_plugins);

                // Get or create the plugins table
                let plugins_table: &mut toml::Table = res
                    .entry(String::from("plugins"))
                    .or_insert_with(|| toml::Table::new().into())
                    .as_table_mut()
                    .context("invalid config entry 'plugins': it should be a table")?;

                // Set "enabled" = true/false depending on the list of enabled plugins.
                for p in plugins {
                    let name = p.name.clone();
                    let enabled = enabled_set.remove(&name);
                    let plugin_table: &mut toml::Table = plugins_table
                        .entry(name)
                        .or_insert_with(|| toml::Table::new().into())
                        .as_table_mut()
                        .with_context(|| format!("invalid config entry plugins.{}", p.name))?;

                    plugin_table.insert(String::from("enable"), toml::Value::Boolean(enabled));
                }

                // Check that all the plugins listed in the argument actually exist
                if !enabled_set.is_empty() {
                    let list = enabled_set.into_iter().collect::<Vec<_>>().join(", ");
                    return Err(anyhow!("Invalid list of plugins to enable: no such plugin(s) {list}"));
                }
            }
            Ok(Some(res))
        }
    }

    impl Configurator for CommonArgs {
        /// Applies the common CLI args to the pipeline.
        fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
            if let Some(max_update_interval) = self.max_update_interval {
                pipeline.trigger_constraints_mut().max_update_interval = max_update_interval;
            }
            if let Some(source_channel_size) = self.source_channel_size {
                *pipeline.source_channel_size() = source_channel_size;
            }
            if let Some(n) = self.normal_worker_threads {
                pipeline.normal_threads(n);
            }
            if let Some(n) = self.priority_worker_threads {
                pipeline.high_priority_threads(n);
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
    /// use alumet_agent::options::config::CommonOpts;
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
        max_update_interval: Option<Duration>,
        source_channel_size: Option<usize>,
    }

    impl Configurator for CommonOpts {
        fn configure_pipeline(&mut self, pipeline: &mut alumet::pipeline::Builder) {
            if let Some(max_update_interval) = self.max_update_interval {
                pipeline.trigger_constraints_mut().max_update_interval = max_update_interval;
            }
            if let Some(source_channel_size) = self.source_channel_size {
                *pipeline.source_channel_size() = source_channel_size;
            }
        }
    }

    impl Default for CommonOpts {
        fn default() -> Self {
            Self {
                max_update_interval: Some(Duration::from_secs(1)),
                source_channel_size: None,
            }
        }
    }
}
