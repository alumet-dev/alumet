//! Helpers for creating a measurement agent.

use std::fmt::Display;

use crate::{
    config::ConfigTable,
    pipeline::{
        self,
        builder::PipelineBuilder,
        runtime::{IdlePipeline, RunningPipeline},
    },
    plugin::{AlumetStart, Plugin, PluginMetadata},
};

/// Easy-to-use skeleton for building a measurement application based on
/// the core of Alumet, aka an "agent".
///
/// Use the [`AgentBuilder`] to build a new agent.
///
/// ## Example
/// ```no_run
/// use alumet::agent::{static_plugins, AgentBuilder, Agent};
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
/// #     fn init(config: &mut alumet::config::ConfigTable) -> anyhow::Result<Box<Self>> {
/// #         todo!()
/// #     }
/// #
/// #     fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
/// #         todo!()
/// #     }
/// #
/// #     fn stop(&mut self) -> anyhow::Result<()> {
/// #         todo!()
/// #     }
/// # }
/// // Extract metadata from plugins (here, only one static plugin).
/// let plugins = static_plugins![PluginA];
///
/// // Parse the configuration file.
/// let config_path = std::path::Path::new("alumet-config.toml");
/// let file_content = std::fs::read_to_string(config_path).expect("failed to read file");
/// let config: toml::Table = file_content.parse().unwrap();
///
/// // Build the agent.
/// let agent: Agent = AgentBuilder::new(plugins, config).build();
/// ```
pub struct Agent {
    settings: AgentBuilder,
}

/// A builder for [`Agent`].
pub struct AgentBuilder {
    plugins: Vec<PluginMetadata>,
    config: toml::Table,
    f_after_plugin_init: fn(&mut Vec<Box<dyn Plugin>>),
    f_after_plugin_start: fn(&PipelineBuilder),
    f_before_operation_begin: fn(&IdlePipeline),
    f_after_operation_begin: fn(&mut RunningPipeline),
}

impl Agent {
    /// Starts the agent.
    ///
    /// This method takes care of the following steps:
    /// - plugin initialization
    /// - plugin start-up
    /// - creation and start-up of the measurement pipeline
    ///
    /// You can be notified after each step by building your agent
    /// with callbacks such as [`AgentBuilder::after_plugin_init`].
    pub fn start(self) -> RunningPipeline {
        // Initialization phase.
        log::info!("Initializing the plugins...");
        let mut global_config = self.settings.config;
        let mut initialized_plugins: Vec<Box<dyn Plugin>> = self
            .settings
            .plugins
            .into_iter()
            .map(|plugin| {
                let name = plugin.name.clone();
                let version = plugin.version.clone();
                initialize_with_config(&mut global_config, plugin).unwrap_or_else(|err| {
                    log_and_panic(err, format!("Plugin failed to initialize: {} v{}", name, version))
                })
            })
            .collect();

        match initialized_plugins.len() {
            0 => log::warn!("No plugin has been initialized, please check your AgentBuilder."),
            1 => log::info!("1 plugin initialized."),
            n => log::info!("{n} plugins initialized."),
        };
        (self.settings.f_after_plugin_init)(&mut initialized_plugins);

        // Start-up phase.
        log::info!("Starting the plugins...");
        let mut pipeline_builder = pipeline::builder::PipelineBuilder::new();
        for plugin in initialized_plugins.iter_mut() {
            log::debug!("Starting plugin {} v{}", plugin.name(), plugin.version());
            let mut start_struct = AlumetStart {
                pipeline_builder: &mut pipeline_builder,
                current_plugin_name: plugin.name().to_owned(),
            };
            plugin.start(&mut start_struct).unwrap_or_else(|err| {
                log_and_panic(
                    err,
                    format!("Plugin failed to start: {} v{}", plugin.name(), plugin.version()),
                )
            })
        }
        print_stats(&pipeline_builder, &initialized_plugins);
        (self.settings.f_after_plugin_start)(&pipeline_builder);

        // Pre-Operation: pipeline building.
        log::info!("Building the measurement pipeline...");
        let pipeline = pipeline_builder.build().unwrap_or_else(|err| {
            log::error!("Pipeline failed to build: {err}");
            panic!("Error: {err}")
        });
        for plugin in initialized_plugins.iter_mut() {
            plugin.pre_pipeline_start(&pipeline).unwrap_or_else(|err| {
                log_and_panic(
                    err,
                    format!(
                        "Plugin pre_pipeline_start failed: {} v{}",
                        plugin.name(),
                        plugin.version()
                    ),
                )
            });
        }
        (self.settings.f_before_operation_begin)(&pipeline);

        log::info!("Starting the measurement pipeline...");
        let mut pipeline = pipeline.start();

        // Operation: the pipeline is running.
        for plugin in initialized_plugins.iter_mut() {
            plugin.post_pipeline_start(&mut pipeline).unwrap_or_else(|err| {
                log_and_panic(
                    err,
                    format!(
                        "Plugin post_pipeline_start failed: {} v{}",
                        plugin.name(),
                        plugin.version()
                    ),
                )
            })
        }

        log::info!("🔥 ALUMET measurement pipeline has started.");
        (self.settings.f_after_operation_begin)(&mut pipeline);

        pipeline
    }
}

fn log_and_panic<M: Display>(error: anyhow::Error, message: M) -> ! {
    // Use the debug format to display all the causes of the error,
    // see https://docs.rs/anyhow/1.0.82/anyhow/struct.Error.html#display-representations.
    log::error!("{message} - {error:?}");
    panic!("{message} - {error}");
}

/// Finds the configuration of a plugin in the global config, and initialize the plugin.
fn initialize_with_config(global_config: &mut toml::Table, plugin: PluginMetadata) -> anyhow::Result<Box<dyn Plugin>> {
    let name = &plugin.name;
    let version = &plugin.version;
    let sub_config = global_config.remove(name);
    let mut plugin_config = match sub_config {
        Some(toml::Value::Table(t)) => Ok(ConfigTable::new(t)?),
        Some(bad_value) => Err(anyhow::anyhow!(
            "invalid configuration for plugin '{name}' v{version}: the value must be a table, not a {}.",
            bad_value.type_str()
        )),
        None => {
            // default to an empty config, so that the plugin can load some default values.
            Ok(ConfigTable::new(toml::map::Map::new())?)
        }
    }?;
    (plugin.init)(&mut plugin_config)
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
    let metrics_list = if metrics.len() == 0 {
        String::from("    ∅")
    } else {
        metrics
            .iter()
            .map(|(_id, m)| format!("    - {}: {} ({})", m.name, m.value_type, m.unit))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let n_sources = pipeline_builder.sources.len();
    let n_transforms = pipeline_builder.transforms.len();
    let n_output = pipeline_builder.outputs.len();
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
    /// The global configuration contains the configuration of each
    /// plugin, as TOML subtables. If a subtable is missing, the plugin
    /// will receive an empty table for its initialization.
    pub fn new(plugins: Vec<PluginMetadata>, config: toml::Table) -> Self {
        Self {
            plugins,
            config,
            f_after_plugin_init: |_| (),
            f_after_plugin_start: |_| (),
            f_before_operation_begin: |_| (),
            f_after_operation_begin: |_| (),
        }
    }

    /// Creates an agent with these settings.
    pub fn build(self) -> Agent {
        Agent { settings: self }
    }

    /// Defines a function to run after the plugin initialization phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_init(&mut self, f: fn(&mut Vec<Box<dyn Plugin>>)) -> &mut Self {
        self.f_after_plugin_init = f;
        self
    }

    /// Defines a function to run after the plugin start-up phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_start(&mut self, f: fn(&PipelineBuilder)) -> &mut Self {
        self.f_after_plugin_start = f;
        self
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn before_operation_begin(&mut self, f: fn(&IdlePipeline)) -> &mut Self {
        self.f_before_operation_begin = f;
        self
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_operation_begin(&mut self, f: fn(&mut RunningPipeline)) -> &mut Self {
        self.f_after_operation_begin = f;
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

pub use static_plugins;

#[cfg(test)]
mod tests {
    use crate::plugin::rust::AlumetPlugin;

    #[test]
    fn static_plugin_macro() {
        let empty = static_plugins![];
        assert!(empty.is_empty());

        let single = static_plugins![MyPlugin];
        assert_eq!(1, single.len());
        assert_eq!("name", single[0].name);
        assert_eq!("version", single[0].version);

        // Accept single identifiers and qualified paths.
        let multiple = static_plugins![MyPlugin, self::MyPlugin];
        assert_eq!(2, multiple.len());
    }

    struct MyPlugin;
    impl AlumetPlugin for MyPlugin {
        fn name() -> &'static str {
            "name"
        }

        fn version() -> &'static str {
            "version"
        }

        fn init(_config: &mut crate::config::ConfigTable) -> anyhow::Result<Box<Self>> {
            todo!()
        }

        fn start(&mut self, _alumet: &mut crate::plugin::AlumetStart) -> anyhow::Result<()> {
            todo!()
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            todo!()
        }
    }
}
