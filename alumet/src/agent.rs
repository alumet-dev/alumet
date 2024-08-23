//! Helpers for creating a measurement agent.
//!
//! # Example
//!
//! ```
//! use alumet::agent::{AgentBuilder, RunningAgent};
//! use std::time::Duration;
//!
//! # fn f() -> anyhow::Result<()> {
//! // create and start an Agent
//! let mut agent = AgentBuilder::new(vec![]).config_value(toml::Table::new()).build();
//! let config = agent.load_config()?;
//! let agent: RunningAgent = agent.start(config)?;
//!
//! // initiate shutdown, this can be done from any thread
//! agent.pipeline.control_handle().shutdown();
//!
//! // run until the shutdown command is processed, and stop all the plugins
//! let timeout = Duration::MAX;
//! agent.wait_for_shutdown(timeout)?;
//! # Ok(())
//! # }
//! ```

use std::{
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use fancy_regex::{Captures, Regex};

use crate::plugin::{AlumetPluginStart, AlumetPostStart, ConfigTable, Plugin, PluginMetadata};
use crate::{
    pipeline::{self, trigger::TriggerConstraints},
    plugin::AlumetPreStart,
};

/// ENV_VAR is a regex which matches every unescaped linux environment
/// i.e. `$VAR` or `$VAR_BIS` for example but not `\$ESCAPED_VAR`
const ENV_VAR: &'static str = r"\w?(?<!\\)\$([[:word:]]*)";

/// Easy-to-use skeleton for building a measurement application based on
/// the core of Alumet, aka an "agent".
///
/// Use the [`AgentBuilder`] to build a new agent.
///
/// # Example
/// ```no_run
/// use alumet::agent::{static_plugins, AgentBuilder, Agent};
/// use alumet::plugin::{AlumetPluginStart, ConfigTable};
/// use alumet::plugin::rust::AlumetPlugin;
///
/// # struct PluginA;
/// # impl AlumetPlugin for PluginA {
/// #     fn name() -> &'static str {
/// #         "name"
/// #     }
/// #
/// #     fn version() -> &'static str {
/// #         "version"
/// #     }
/// #
/// #     fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
/// #         todo!()
/// #     }
/// #
/// #     fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
/// #         todo!()
/// #     }
/// #
/// #     fn stop(&mut self) -> anyhow::Result<()> {
/// #         todo!()
/// #     }
/// # }
/// // Extract metadata from plugins (here just one static plugin, which implements AlumetPlugin).
/// let plugins = static_plugins![PluginA];
///
/// // Locate the configuration file.
/// let config_path = std::path::Path::new("alumet-config.toml");
///
/// // Build the agent.
/// let agent: Agent = AgentBuilder::new(plugins).config_path(config_path).build();
/// ```
pub struct Agent {
    settings: AgentBuilder,
}

/// An Agent that has been started.
pub struct RunningAgent {
    pub pipeline: pipeline::MeasurementPipeline,
    initialized_plugins: Vec<Box<dyn Plugin>>,
}

/// A builder for [`Agent`].
pub struct AgentBuilder {
    plugins: Vec<PluginMetadata>,
    config: Option<AgentConfigSource>,
    default_app_config: toml::Table,
    f_after_plugins_init: Box<dyn FnOnce(&mut Vec<Box<dyn Plugin>>)>,
    f_after_plugins_start: Box<dyn FnOnce(&pipeline::Builder)>,
    f_before_operation_begin: Box<dyn FnOnce(&pipeline::Builder)>,
    f_after_operation_begin: Box<dyn FnOnce(&mut pipeline::MeasurementPipeline)>,
    no_high_priority_threads: bool,
    source_constraints: TriggerConstraints,
}

/// Where to get the configuration from.
enum AgentConfigSource {
    /// Use this toml table as the agent configuration.
    Value(toml::Table),
    /// Read the configuration from a file.
    FilePath(std::path::PathBuf),
}

/// Agent configuration.
pub struct AgentConfig {
    /// Contains the configuration of each plugin (one subtable per plugin).
    plugins_table: toml::Table,
    /// Contains the configuration of the agent application.
    ///
    /// This is an Option in order to allow [`Option::take`].
    app_table: Option<toml::Table>,
}

impl TryFrom<toml::Table> for AgentConfig {
    type Error = anyhow::Error;

    /// Parses a TOML configuration into an agent configuration.
    ///
    /// # Required structure
    ///
    /// For the agent configuration to be valid, it must contain a `plugins` table,
    /// with one subtable per plugin. The rest of the configuration (outside of the
    /// `plugins` table), will be used by the agent application.
    fn try_from(mut global_config: toml::Table) -> Result<Self, Self::Error> {
        // Extract the plugins' configurations.
        let plugins_table = match global_config.remove("plugins") {
            Some(toml::Value::Table(t)) => Ok(t),
            Some(bad_value) => Err(anyhow!(
                "invalid global config: 'plugins' must be a table, not a {}.",
                bad_value.type_str()
            )),
            None => Err(anyhow!("invalid global config: it should contain a 'plugins' table")),
        }?;

        // What remains is the app's configuration.
        let app_table = Some(global_config);

        Ok(AgentConfig {
            plugins_table,
            app_table,
        })
    }
}

impl AgentConfig {
    /// Returns a mutable reference to the plugin's subconfig, which is at `plugins.<name>`
    pub fn plugin_config_mut(&mut self, plugin_name: &str) -> Option<&mut toml::Table> {
        let sub_config = self.plugins_table.get_mut(plugin_name);
        match sub_config {
            Some(toml::Value::Table(t)) => Some(t),
            _ => None,
        }
    }

    /// Takes the plugin's subconfig, which is at `plugins.<name>`
    ///
    /// See [`Option::take`].
    pub fn take_plugin_config(&mut self, plugin_name: &str) -> anyhow::Result<toml::Table> {
        let sub_config = self.plugins_table.remove(plugin_name);
        match sub_config {
            Some(toml::Value::Table(t)) => Ok(t),
            Some(bad_value) => Err(anyhow!(
                "invalid configuration for plugin '{plugin_name}': the value must be a table, not a {}.",
                bad_value.type_str()
            )),
            None => {
                // default to an empty config, so that the plugin can load some default values.
                Ok(toml::Table::new())
            }
        }
    }

    /// Returns a mutable reference to the agent app config.
    pub fn app_config_mut(&mut self) -> &mut toml::Table {
        self.app_table.as_mut().unwrap()
    }

    /// Takes the agent app config.
    ///
    /// See [`Option::take`].
    pub fn take_app_config(&mut self) -> toml::Table {
        self.app_table.take().unwrap()
    }
}

impl Agent {
    /// Tries to load the configuration from its source (as defined by [`AgentBuilder::config_value`] or [`AgentBuilder::config_path`]).
    pub fn load_config(&mut self) -> anyhow::Result<AgentConfig> {
        // Load the global config, from a file or from a value, depending on the agent's settings.
        let global_config = match self.settings.config.take().unwrap() {
            AgentConfigSource::Value(config) => config,
            AgentConfigSource::FilePath(path) => {
                load_config_from_file(&self.settings.plugins, &path, &self.settings.default_app_config)?
            }
        };
        log::debug!("Global configuration: {global_config:?}");

        // Wrap the config in AgentConfig and check its structure.
        let config = AgentConfig::try_from(global_config).context("invalid agent configuration")?;
        Ok(config)
    }

    /// Starts the agent.
    ///
    /// This method takes care of the following steps:
    /// - plugin initialization
    /// - plugin start-up
    /// - creation and start-up of the measurement pipeline
    ///
    /// You can be notified after each step by building your agent
    /// with callbacks such as [`AgentBuilder::after_plugin_init`].
    #[must_use = "To keep Alumet running, call RunningAgent::wait_for_shutdown."]
    pub fn start(self, mut config: AgentConfig) -> anyhow::Result<RunningAgent> {
        // Initialization phase.
        log::info!("Initializing the plugins...");

        // initialize the plugins with the config
        let mut initialized_plugins: Vec<Box<dyn Plugin>> = self
            .settings
            .plugins
            .into_iter()
            .map(|plugin| -> anyhow::Result<Box<dyn Plugin>> {
                let name = plugin.name.clone();
                let version = plugin.version.clone();
                initialize_with_config(&mut config, plugin)
                    .with_context(|| format!("Plugin failed to initialize: {} v{}", name, version))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        match initialized_plugins.len() {
            0 => log::warn!("No plugin has been initialized, please check your AgentBuilder."),
            1 => log::info!("1 plugin initialized."),
            n => log::info!("{n} plugins initialized."),
        };
        (self.settings.f_after_plugins_init)(&mut initialized_plugins);

        // Start-up phase.
        log::info!("Starting the plugins...");
        let mut pipeline_builder = pipeline::Builder::new();
        pipeline_builder.set_trigger_constraints(self.settings.source_constraints);
        let mut post_start_actions = Vec::new();

        // Disable high-priority threads if asked to
        if self.settings.no_high_priority_threads {
            pipeline_builder.high_priority_threads(0);
        }

        // Call start(AlumetPluginStart) on each plugin.
        for plugin in initialized_plugins.iter_mut() {
            log::debug!("Starting plugin {} v{}", plugin.name(), plugin.version());
            let mut start_context = AlumetPluginStart {
                pipeline_builder: &mut pipeline_builder,
                current_plugin: pipeline::PluginName(plugin.name().to_owned()),
                post_start_actions: &mut post_start_actions,
            };
            plugin
                .start(&mut start_context)
                .with_context(|| format!("Plugin failed to start: {} v{}", plugin.name(), plugin.version()))?;
        }
        print_stats(&pipeline_builder, &initialized_plugins);
        (self.settings.f_after_plugins_start)(&pipeline_builder);

        // Call pre_pipeline_start(AlumetPreStart) on each plugin.
        log::info!("Running pre-pipeline-start hooks...");
        for plugin in initialized_plugins.iter_mut() {
            let mut ctx = AlumetPreStart {
                current_plugin: pipeline::PluginName(plugin.name().to_owned()),
                pipeline_builder: &mut pipeline_builder,
            };
            plugin.pre_pipeline_start(&mut ctx).with_context(|| {
                format!(
                    "Plugin pre_pipeline_start failed: {} v{}",
                    plugin.name(),
                    plugin.version()
                )
            })?;
        }
        (self.settings.f_before_operation_begin)(&pipeline_builder);

        // Operation: the pipeline is running.
        log::info!("Starting the measurement pipeline...");
        let mut pipeline = pipeline_builder.build().context("Pipeline failed to build")?;
        log::info!("🔥 ALUMET measurement pipeline has started.");

        // Call post_pipeline_start(AlumetPostStart) on each plugin.
        log::info!("Running post-pipeline-start hooks...");
        for (plugin, action) in post_start_actions {
            let mut ctx = AlumetPostStart {
                current_plugin: plugin.clone(),
                pipeline: &mut pipeline,
            };
            action(&mut ctx).with_context(|| format!("Error in post-pipeline-start action of plugin {}", plugin.0))?;
        }
        for plugin in initialized_plugins.iter_mut() {
            let mut ctx = AlumetPostStart {
                current_plugin: pipeline::PluginName(plugin.name().to_owned()),
                pipeline: &mut pipeline,
            };
            plugin.post_pipeline_start(&mut ctx).with_context(|| {
                format!(
                    "Plugin post_pipeline_start failed: {} v{}",
                    plugin.name(),
                    plugin.version()
                )
            })?;
        }

        (self.settings.f_after_operation_begin)(&mut pipeline);
        log::info!("🔥 ALUMET agent is ready.");

        let agent = RunningAgent {
            pipeline,
            initialized_plugins,
        };
        Ok(agent)
    }

    /// Builds a default configuration by combining:
    /// - the default agent config (which is set by [`AgentBuilder::default_app_config`])
    /// - the default config of each plugin (which are set by [`AgentBuilder::new`])
    pub fn default_config(&self) -> anyhow::Result<toml::Table> {
        build_default_config(&self.settings.plugins, &self.settings.default_app_config)
    }

    /// Builds and saves a default configuration by combining:
    /// - the default agent config (which is set by [`AgentBuilder::default_app_config`])
    /// - the default config of each plugin (which are set by [`AgentBuilder::new`])
    ///
    /// This can be used to provide a command line option that (re)generates the configuration file.
    pub fn write_default_config(&self) -> anyhow::Result<()> {
        let default_config = self.default_config()?;
        match self.settings.config.as_ref().unwrap() {
            AgentConfigSource::Value(_) => Err(anyhow!(
                "write_default_config() only works if the Agent is built with config_path()"
            )),
            AgentConfigSource::FilePath(path) => {
                std::fs::write(path, default_config.to_string())
                    .with_context(|| format!("writing default config to {}", path.display()))?;
                Ok(())
            }
        }
    }

    /// Sets the maximum interval between two updates of the commands processed by
    /// each measurement [`Source`](crate::pipeline::Source).
    ///
    /// This only applies to the sources that are triggered by a time interval
    /// managed by Alumet.
    pub fn sources_max_update_interval(&mut self, max_update_interval: Duration) {
        self.settings.source_constraints.max_update_interval = max_update_interval;
    }
}

impl RunningAgent {
    /// Waits until the measurement pipeline stops, then stops the plugins.
    ///
    /// See the [module documentation](super::agent).
    pub fn wait_for_shutdown(self, timeout: Duration) -> anyhow::Result<()> {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let mut n_errors = 0;

        // Tokio's timeout has a maximum timeout that is much smaller than Duration::MAX,
        // and will replace the latter by its maximum timeout.
        // Therefore, we use an Option to disable the timeout if it's Duration::MAX.
        let timeout = Some(timeout).filter(|d| *d != Duration::MAX);

        // Wait for the pipeline to be stopped, by Ctrl+C or a command.
        // Also, **drop** the pipeline before stopping the plugin, because Plugin::stop expects
        // the sources, transforms and outputs to be stopped and dropped before it is called.
        // All tokio tasks that have not finished yet will abort.
        match self.pipeline.wait_for_shutdown(timeout) {
            Ok(Ok(_)) => (),
            Ok(Err(err)) => {
                log::error!("Error in the measurement pipeline: {err:?}");
                n_errors += 1;
            }
            Err(_elapsed) => {
                log::error!(
                    "Timeout of {:?} expired while waiting for the pipeline to shut down",
                    timeout.unwrap()
                );
                n_errors += 1;
            }
        }

        // Stop all the plugins, even if some of them fail to stop properly.
        log::info!("Stopping the plugins...");
        for mut plugin in self.initialized_plugins {
            let name = plugin.name().to_owned();
            let version = plugin.version().to_owned();
            log::info!("Stopping plugin {name} v{version}");

            // If a plugin panics, we still want to try to stop the other plugins.
            match catch_unwind(AssertUnwindSafe(move || {
                plugin.stop()
                // plugin is dropped here
            })) {
                Ok(Ok(())) => (),
                Ok(Err(e)) => {
                    log::error!("Error while stopping plugin {name} v{version}. {e:#}");
                    n_errors += 1;
                }
                Err(panic_payload) => {
                    log::error!(
                        "PANIC while stopping plugin {name} v{version}. There is probably a bug in the plugin!
                        Please check the implementation of stop (and drop if Drop is implemented for the plugin type)."
                    );
                    n_errors += 1;
                    // dropping the panic payload may, in turn, panic!
                    let _ = catch_unwind(AssertUnwindSafe(move || {
                        drop(panic_payload);
                    }))
                    .map_err(|panic2| {
                        log::error!(
                            "PANIC while dropping panic payload generated while stopping plugin {name} v{version}."
                        );
                        // We cannot drop it, forget it.
                        // Alumet will stop after this anyway, but the plugin should be fixed.
                        std::mem::forget(panic2);
                    });
                }
            }
        }
        log::info!("All plugins have stopped.");

        if n_errors == 0 {
            Ok(())
        } else {
            let error_str = if n_errors == 1 { "error" } else { "errors" };
            Err(anyhow!("{n_errors} {error_str} occurred during the shutdown phase"))
        }
    }
}

/// Loads a configuration from a TOML file.
///
/// If the file does not exist, attempts to create it with a default configuration.
fn load_config_from_file(
    plugins: &[PluginMetadata],
    path: &Path,
    default_agent_config: &toml::Table,
) -> anyhow::Result<toml::Table> {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let config: Result<Map<String, Value>, anyhow::Error> = Regex::new(ENV_VAR)
                .expect("Invalid Regex")
                .replace_all(&content, |c: &Captures| match &c[1] {
                    // If the captured string is `$` then we manage it as an escaped `$`.
                    "" => String::from("$"),
                    // Else, we replace the `$varname` by its value. If the env var is empty or does
                    // not exist, then we raise a panic.
                    varname => env::var(varname).expect(format!("{varname} is invalid or empty").as_str()),
                })
                .to_owned()
                .parse()
                .with_context(|| format!("invalid TOML configuration {}", path.display()));

            log::info!("Loading config: {}", path.display());
            config
        }
        Err(e) => {
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    // the file does not exist, create the default config and save it
                    let default_config = build_default_config(plugins, default_agent_config)?;
                    std::fs::write(path, default_config.to_string())
                        .with_context(|| format!("writing default config to {}", path.display()))?;
                    log::info!("Default configuration written to {}", path.display());
                    Ok(default_config)
                }
                _ => Err(anyhow!(
                    "unable to load the configuration from {} - {e}",
                    path.display()
                )),
            }
        }
    }
}

/// Builds a default global configuration from the default configs of all the plugins,
/// and the default config of the agent.
fn build_default_config(plugins: &[PluginMetadata], default_agent_config: &toml::Table) -> anyhow::Result<toml::Table> {
    let mut default_config = default_agent_config.clone();

    // Fill the config with all the default configs of the plugins,
    // in a subtable to avoid name conflicts with the agent config.
    let mut plugins_config = toml::Table::new();
    for plugin in plugins {
        log::debug!("Generating default config for plugin {}", plugin.name);
        let default_plugin_config = (plugin.default_config)()?;
        log::debug!("default config: {default_plugin_config:?}");
        if let Some(conf) = default_plugin_config {
            let key = plugin.name.clone();
            plugins_config.insert(key, toml::Value::Table(conf.0));
        }
    }
    default_config.insert(String::from("plugins"), toml::Value::Table(plugins_config));
    Ok(default_config)
}

/// Finds the configuration of a plugin in the global config, and initialize the plugin.
fn initialize_with_config(agent_config: &mut AgentConfig, plugin: PluginMetadata) -> anyhow::Result<Box<dyn Plugin>> {
    let name = &plugin.name;
    let plugin_config = agent_config.take_plugin_config(name)?;

    log::debug!("Initializing plugin {name} with config {plugin_config:?}");
    (plugin.init)(ConfigTable(plugin_config))
}

/// Prints some statistics after the plugin start-up phase.
fn print_stats(pipeline_builder: &pipeline::Builder, plugins: &[Box<dyn Plugin>]) {
    // plugins
    let plugins_list = plugins
        .iter()
        .map(|p| format!("    - {} v{}", p.name(), p.version()))
        .collect::<Vec<_>>()
        .join("\n");

    let metrics = &pipeline_builder.metrics;
    let metrics_list = if metrics.is_empty() {
        String::from("    ∅")
    } else {
        let mut m = metrics
            .iter()
            .map(|(id, m)| (id, format!("    - {}: {} ({})", m.name, m.value_type, m.unit)))
            .collect::<Vec<_>>();
        // Sort by metric id to display the metrics in the order they were registered (less confusing).
        m.sort_by_key(|(id, _)| id.0);
        m.into_iter()
            .map(|(_, metric_str)| metric_str)
            .collect::<Vec<_>>()
            .join("\n")
    };

    let stats = pipeline_builder.stats();
    let str_source = if stats.sources > 1 { "sources" } else { "source" };
    let str_transform = if stats.transforms > 1 {
        "transforms"
    } else {
        "transform"
    };
    let str_output = if stats.outputs > 1 { "outputs" } else { "output" };
    let pipeline_elements = format!(
        "📥 {} {str_source}, 🔀 {} {str_transform} and 📝 {} {str_output} registered.",
        stats.sources, stats.transforms, stats.outputs,
    );

    let n_plugins = plugins.len();
    let n_metrics = stats.metrics;
    let str_plugin = if n_plugins > 1 { "plugins" } else { "plugin" };
    let str_metric = if n_metrics > 1 { "metrics" } else { "metric" };
    log::info!("Plugin startup complete.\n🧩 {n_plugins} {str_plugin} started:\n{plugins_list}\n📏 {n_metrics} {str_metric} registered:\n{metrics_list}\n{pipeline_elements}");
}

impl AgentBuilder {
    /// Creates a new builder with some non-initialized plugins,
    /// and the global configuration of the agent.
    ///
    // /// The global configuration contains the configuration of each
    // /// plugin, as TOML subtables. If a subtable is missing, the plugin
    // /// will receive an empty table for its initialization.
    pub fn new(plugins: Vec<PluginMetadata>) -> Self {
        Self {
            plugins,
            config: Some(AgentConfigSource::FilePath(PathBuf::from("alumet-config.toml"))),
            default_app_config: toml::Table::new(),
            f_after_plugins_init: Box::new(|_| ()),
            f_after_plugins_start: Box::new(|_| ()),
            f_before_operation_begin: Box::new(|_| ()),
            f_after_operation_begin: Box::new(|_| ()),
            no_high_priority_threads: false,
            source_constraints: TriggerConstraints::default(),
        }
    }

    /// Creates an agent with these settings.
    pub fn build(self) -> Agent {
        Agent { settings: self }
    }

    /// Loads the global configuration from the given file path.
    pub fn config_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.config = Some(AgentConfigSource::FilePath(path.as_ref().to_owned()));
        self
    }

    /// Defines the default configuration for the agent application (not the plugins).
    pub fn default_app_config_table(mut self, app_config: toml::Table) -> Self {
        self.default_app_config = app_config;
        self
    }

    /// Defines the default configuration for the agent application (not the plugins).
    ///
    /// If `app_config` cannot be serialized to a [`toml::Table`], this function will panic.
    pub fn default_app_config<C: Serialize>(mut self, app_config: C) -> Self {
        self.default_app_config =
            toml::Table::try_from(app_config).expect("default app config should be serializable to a TOML table");
        self
    }

    /// Uses the given table as the global configuration.
    ///
    /// Use this method to provide the configuration yourself instead of loading
    /// it from a file. For instance, this can be used to load the configuration
    /// from the command line arguments.
    pub fn config_value(mut self, config: toml::Table) -> Self {
        self.config = Some(AgentConfigSource::Value(config));
        self
    }

    /// Defines a function to run after the plugin initialization phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_init<F: FnOnce(&mut Vec<Box<dyn Plugin>>) + 'static>(mut self, f: F) -> Self {
        self.f_after_plugins_init = Box::new(f);
        self
    }

    /// Defines a function to run after the plugin start-up phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_start<F: FnOnce(&pipeline::Builder) + 'static>(mut self, f: F) -> Self {
        self.f_after_plugins_start = Box::new(f);
        self
    }

    // Defines a function to run after the plugins have started but before the pipeline starts.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn before_operation_begin<F: FnOnce(&pipeline::Builder) + 'static>(mut self, f: F) -> Self {
        self.f_before_operation_begin = Box::new(f);
        self
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_operation_begin<F: FnOnce(&mut pipeline::MeasurementPipeline) + 'static>(mut self, f: F) -> Self {
        self.f_after_operation_begin = Box::new(f);
        self
    }

    /// Disables "high priority" threads.
    ///
    /// Use this when you are in an environment that you know will not allow Alumet to increase the scheduling priority of its threads.
    pub fn no_high_priority_threads(mut self) -> Self {
        self.no_high_priority_threads = true;
        self
    }
}

/// Creates a [`Vec`] containing [`PluginMetadata`] for static plugins.
///
/// Each argument must be a _type_ that implements the [`AlumetPlugin`](crate::plugin::rust::AlumetPlugin) trait.
///
/// # Example
/// ```ignore
/// use alumet::plugin::PluginMetadata;
///
/// let plugins: Vec<PluginMetadata> = static_plugins![PluginA, PluginB];
/// ```
#[macro_export]
macro_rules! static_plugins {
    // ```
    // static_plugins![MyPluginA, ...];
    // ```
    //
    // desugars to:
    // ```
    // let plugins = vec![PluginMetadata::from_static::<MyPlugin>(), ...]
    // ```
    [] => {
        Vec::<$crate::plugin::PluginMetadata>::new()
    };
    [$($x:path),*] => {
        {
            vec![
                $(
                    $crate::plugin::PluginMetadata::from_static::<$x>(),
                )*
            ]
        }
    }
}

use anyhow::{anyhow, Context};
use serde::Serialize;
pub use static_plugins;
use toml::{map::Map, Value};

#[cfg(test)]
mod tests {
    use std::env;

    use fancy_regex::Regex;
    use serde::Serialize;

    use crate::plugin::rust::{serialize_config, AlumetPlugin};
    use crate::plugin::{AlumetPluginStart, ConfigTable};

    use super::ENV_VAR;

    #[test]
    fn parse_config_file() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("test-config.toml");
        let config_content = r#"
        key = "value"
        
        [plugins.name]
        list = ["a", "b"]
        count = 1
    "#;
        std::fs::write(&config_path, config_content).unwrap();

        let plugins = static_plugins![MyPlugin];
        let config = super::load_config_from_file(&plugins, &config_path, &toml::Table::new()).unwrap();
        assert_eq!(
            config,
            config_content.parse::<toml::Table>().unwrap(),
            "returned config is wrong"
        );
        assert_eq!(
            std::fs::read_to_string(config_path).unwrap(),
            config_content,
            "config file should not change"
        );
    }

    #[test]
    fn parse_config_file_with_env_var() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("test-config-with-env-var.toml");
        // Ensuring that an escaped `$` or a raw `$`, i.e. without any name,
        // stay and is not replaced by anything. `$HOST:$PORT` should be
        // replaced correctly because we are setting their values below.
        let config_content = r#"
        key = "value"
        
        [plugins.name]
        list = ["a $", "\\$b"]
        url = "http://$HOST:$PORT"
    "#;
        std::fs::write(&config_path, config_content).unwrap();

        env::set_var("HOST", "8.8.8.8");
        env::set_var("PORT", "42");

        let plugins = static_plugins![MyPlugin];
        let config = super::load_config_from_file(&plugins, &config_path, &toml::Table::new()).unwrap();

        assert_eq!(
            config,
            config_content
                .replace("$HOST", "8.8.8.8")
                .replace("$PORT", "42")
                .parse::<toml::Table>()
                .unwrap(),
            "returned config is wrong"
        );
    }

    #[test]
    #[should_panic]
    fn parse_config_file_with_unexisting_env_var() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("test-config-with-wrong-env-var.toml");
        let config_content = r#"
        key = "value"
        
        [plugins.name]
        list = ["a", "b"]
        url = "$URL_BIS"
    "#;
        std::fs::write(&config_path, config_content).unwrap();

        let plugins = static_plugins![MyPlugin];

        // URL_BIS doesn't exist, load_config_from_file should panic.
        let _ = super::load_config_from_file(&plugins, &config_path, &toml::Table::new()).unwrap();
    }

    #[test]
    fn validate_env_var_regex() {
        let compiled_ENV_VAR = Regex::new(ENV_VAR).expect("Invalid Regex");
        // Verifying that the ENV_VAR regex is still working.
        let mut result = compiled_ENV_VAR.find("abc $VAR abc");

        assert!(result.is_ok(), "execution wasn't successful");
        let mut match_option = result.unwrap();

        assert!(match_option.is_some(), "did not found a match");
        let mut m = match_option.unwrap();

        assert_eq!(m.as_str(), "$VAR");

        result = compiled_ENV_VAR.find("abc $VAR_BIS abc");

        assert!(result.is_ok(), "execution wasn't successful");
        match_option = result.unwrap();

        assert!(match_option.is_some(), "did not found a match");
        m = match_option.unwrap();

        assert_eq!(m.as_str(), "$VAR_BIS");

        result = compiled_ENV_VAR.find(r"abc \$VAR_BIS abc");

        assert!(result.is_ok(), "execution wasn't successful");
        match_option = result.unwrap();

        assert!(
            match_option.is_none(),
            "The regex didn't ignored the escaped environment variable"
        );
    }

    #[test]
    fn create_default_config_file() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("I-do-not-exist.toml");
        let _ = std::fs::remove_file(&config_path);

        let plugins = static_plugins![MyPlugin];
        let config = super::load_config_from_file(&plugins, &config_path, &toml::Table::new()).unwrap();
        let expected: toml::Table = r#"
            [plugins.name]
            list = ["default-item"]
            count = 42
        "#
        .parse()
        .unwrap();
        assert_eq!(config, expected, "returned config is wrong");
        assert!(
            config_path.exists(),
            "config file should be created with default values"
        );
        assert_eq!(
            std::fs::read_to_string(config_path)
                .unwrap()
                .parse::<toml::Table>()
                .unwrap(),
            expected,
            "config file should be correct"
        );
    }

    struct MyPlugin;
    impl AlumetPlugin for MyPlugin {
        fn name() -> &'static str {
            "name"
        }

        fn version() -> &'static str {
            "version"
        }

        fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
            todo!()
        }

        fn start(&mut self, _alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
            todo!()
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            todo!()
        }

        fn default_config() -> anyhow::Result<Option<ConfigTable>> {
            let config = serialize_config(MyPluginConfig::default())?;
            Ok(Some(config))
        }
    }

    #[derive(Serialize)]
    struct MyPluginConfig {
        list: Vec<String>,
        count: u32,
    }

    impl Default for MyPluginConfig {
        fn default() -> Self {
            Self {
                list: vec![String::from("default-item")],
                count: 42,
            }
        }
    }
}
