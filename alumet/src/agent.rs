//! Helpers for creating a measurement agent.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    pipeline::{
        self,
        builder::PipelineBuilder,
        runtime::{IdlePipeline, RunningPipeline},
        trigger::TriggerConstraints,
    },
    plugin::{AlumetStart, ConfigTable, Plugin, PluginMetadata},
};

/// Easy-to-use skeleton for building a measurement application based on
/// the core of Alumet, aka an "agent".
///
/// Use the [`AgentBuilder`] to build a new agent.
///
/// ## Example
/// ```no_run
/// use alumet::agent::{static_plugins, AgentBuilder, Agent};
/// use alumet::plugin::{AlumetStart, ConfigTable};
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
/// #     fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
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
    pub pipeline: RunningPipeline,
    initialized_plugins: Vec<Box<dyn Plugin>>,
}

/// A builder for [`Agent`].
pub struct AgentBuilder {
    plugins: Vec<PluginMetadata>,
    config: Option<AgentConfigSource>,
    default_app_config: toml::Table,
    f_after_plugin_init: fn(&mut Vec<Box<dyn Plugin>>),
    f_after_plugin_start: fn(&PipelineBuilder),
    f_before_operation_begin: fn(&IdlePipeline),
    f_after_operation_begin: fn(&mut RunningPipeline),
    allow_no_metrics: bool,
    source_constraints: TriggerConstraints,
}

enum AgentConfigSource {
    Value(toml::Table),
    FilePath(std::path::PathBuf),
}

pub struct AgentConfig {
    plugins_table: toml::Table,
    app_table: Option<toml::Table>,
}

impl TryFrom<toml::Table> for AgentConfig {
    type Error = anyhow::Error;

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
    /// Returns a mutable reference to the plugin's subconfig, which is at plugins.\<name\>
    pub fn plugin_config_mut(&mut self, plugin_name: &str) -> Option<&mut toml::Table> {
        let sub_config = self.plugins_table.get_mut(plugin_name);
        match sub_config {
            Some(toml::Value::Table(t)) => Some(t),
            _ => None,
        }
    }

    /// Removes and returns the plugin's subconfig, which is at plugins.\<name\>
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

    pub fn app_config_mut(&mut self) -> &mut toml::Table {
        self.app_table.as_mut().unwrap()
    }

    pub fn take_app_config(&mut self) -> toml::Table {
        self.app_table.take().unwrap()
    }
}

impl Agent {
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
        (self.settings.f_after_plugin_init)(&mut initialized_plugins);

        // Start-up phase.
        log::info!("Starting the plugins...");
        let mut pipeline_builder = pipeline::builder::PipelineBuilder::new();
        pipeline_builder.source_constraints = self.settings.source_constraints;
        pipeline_builder.allow_no_metrics = self.settings.allow_no_metrics;

        for plugin in initialized_plugins.iter_mut() {
            log::debug!("Starting plugin {} v{}", plugin.name(), plugin.version());
            let mut start_struct = AlumetStart {
                pipeline_builder: &mut pipeline_builder,
                current_plugin_name: plugin.name().to_owned(),
            };
            plugin
                .start(&mut start_struct)
                .with_context(|| format!("Plugin failed to start: {} v{}", plugin.name(), plugin.version()))?;
        }
        print_stats(&pipeline_builder, &initialized_plugins);
        (self.settings.f_after_plugin_start)(&pipeline_builder);

        // Pre-Operation: pipeline building.
        log::info!("Building the measurement pipeline...");
        let pipeline = pipeline_builder.build().context("Pipeline failed to build")?;
        for plugin in initialized_plugins.iter_mut() {
            plugin.pre_pipeline_start(&pipeline).with_context(|| {
                format!(
                    "Plugin pre_pipeline_start failed: {} v{}",
                    plugin.name(),
                    plugin.version()
                )
            })?;
        }
        (self.settings.f_before_operation_begin)(&pipeline);

        log::info!("Starting the measurement pipeline...");
        let mut pipeline = pipeline.start();

        // Operation: the pipeline is running.
        for plugin in initialized_plugins.iter_mut() {
            plugin.post_pipeline_start(&mut pipeline).with_context(|| {
                format!(
                    "Plugin post_pipeline_start failed: {} v{}",
                    plugin.name(),
                    plugin.version()
                )
            })?;
        }

        log::info!("🔥 ALUMET measurement pipeline has started.");
        (self.settings.f_after_operation_begin)(&mut pipeline);

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
    /// If an element of the pipeline returns an error or panicks, the other elements are aborted and an error is returned.
    pub fn wait_for_shutdown(self) -> anyhow::Result<()> {
        let mut n_errors = 0;

        // Wait for the pipeline to be stopped, by Ctrl+C or a command.
        // Also, **drop** the pipeline before stopping the plugin, because Plugin::stop expects
        // the sources, transforms and outputs to be stopped and dropped before it is called.
        // All tokio tasks that have not finished yet will abort.
        if let Err(err) = self.pipeline.wait_for_shutdown() {
            log::error!("Error in the measurement pipeline: {err}");
            n_errors += 1;
        }

        // Stop all the plugins, even if some of them fail to stop properly.
        log::info!("Stopping the plugins...");
        for mut plugin in self.initialized_plugins {
            let name = plugin.name().to_owned();
            let version = plugin.version().to_owned();
            log::info!("Stopping plugin {name} v{version}");

            if let Err(error) = plugin.stop() {
                log::error!("Error while stopping plugin {name} v{version} - {error:#}");
                n_errors += 1;
            }
        }
        log::info!("All plugins have stopped.");

        if n_errors == 0 {
            Ok(())
        } else {
            let error_str = if n_errors == 1 { "error" } else { "errors" };
            Err(anyhow!("{n_errors} {error_str} occured during the shutdown phase"))
        }
    }
}

fn load_config_from_file(
    plugins: &[PluginMetadata],
    path: &Path,
    default_agent_config: &toml::Table,
) -> anyhow::Result<toml::Table> {
    match std::fs::read_to_string(path) {
        Ok(content) => content
            .parse()
            .with_context(|| format!("invalid TOML configuration {}", path.display())),
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
fn print_stats(pipeline_builder: &PipelineBuilder, plugins: &[Box<dyn Plugin>]) {
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

    let n_sources = pipeline_builder.source_count();
    let n_transforms = pipeline_builder.transform_count();
    let n_output = pipeline_builder.output_count();
    let str_source = if n_sources > 1 { "sources" } else { "source" };
    let str_transform = if n_sources > 1 { "transforms" } else { "transform" };
    let str_output = if n_sources > 1 { "outputs" } else { "output" };
    let pipeline_elements = format!(
        "📥 {} {str_source}, 🔀 {} {str_transform} and 📝 {} {str_output} registered.",
        n_sources, n_transforms, n_output,
    );

    let n_plugins = plugins.len();
    let n_metrics = pipeline_builder.metric_count();
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
            f_after_plugin_init: |_| (),
            f_after_plugin_start: |_| (),
            f_before_operation_begin: |_| (),
            f_after_operation_begin: |_| (),
            allow_no_metrics: false,
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
    pub fn after_plugin_init(mut self, f: fn(&mut Vec<Box<dyn Plugin>>)) -> Self {
        self.f_after_plugin_init = f;
        self
    }

    /// Defines a function to run after the plugin start-up phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_start(mut self, f: fn(&PipelineBuilder)) -> Self {
        self.f_after_plugin_start = f;
        self
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn before_operation_begin(mut self, f: fn(&IdlePipeline)) -> Self {
        self.f_before_operation_begin = f;
        self
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_operation_begin(mut self, f: fn(&mut RunningPipeline)) -> Self {
        self.f_after_operation_begin = f;
        self
    }

    /// Disables the "no metrics registered" warning.
    ///
    /// Use this if you only expect late metrics to be registered.
    pub fn allow_no_metrics(mut self) -> Self {
        self.allow_no_metrics = true;
        self
    }
}

/// Creates a [`Vec`] containing [`PluginMetadata`] for static plugins.
///
/// Each argument must be a _type_ that implements the [`AlumetPlugin`](crate::plugin::rust::AlumetPlugin) trait.
///
/// ## Example
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

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use crate::plugin::rust::{serialize_config, AlumetPlugin};
    use crate::plugin::{AlumetStart, ConfigTable};

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

        fn start(&mut self, _alumet: &mut AlumetStart) -> anyhow::Result<()> {
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
