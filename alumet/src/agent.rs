//! Helpers for creating a measurement agent.

use crate::{
    pipeline::{
        self,
        runtime::{MeasurementPipeline, RunningPipeline},
    },
    plugin::{
        manage::{PluginInitialization, PluginStartup},
        Plugin, PluginMetadata,
    },
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
    f_after_plugin_start: fn(&mut PluginStartup),
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
        let mut init = PluginInitialization::new(self.settings.config);
        let mut initialized_plugins: Vec<Box<dyn Plugin>> = self
            .settings
            .plugins
            .into_iter()
            .map(|plugin| {
                let name = plugin.name.clone();
                let version = plugin.version.clone();
                init.initialize(plugin)
                    .unwrap_or_else(|err| panic!("Plugin failed to initialize: {} v{} - {err}", name, version))
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
        let mut startup = PluginStartup::new();
        for plugin in initialized_plugins.iter_mut() {
            log::debug!("Starting plugin {} v{}", plugin.name(), plugin.version());
            startup.start(plugin.as_mut()).unwrap_or_else(|err| {
                panic!(
                    "Plugin failed to start: {} v{} - {err}",
                    plugin.name(),
                    plugin.version()
                )
            })
        }

        // Post start-up reactions.
        print_stats(&startup, &initialized_plugins);
        for plugin in initialized_plugins.iter_mut() {
            plugin.post_startup(&startup).unwrap_or_else(|err| {
                panic!(
                    "Plugin post-startup action failed: {} v{} - {err}",
                    plugin.name(),
                    plugin.version()
                )
            });
        }
        (self.settings.f_after_plugin_start)(&mut startup);

        // Operation phase.
        log::info!("Starting the measurement pipeline...");
        let mut pipeline = MeasurementPipeline::with_settings(startup.pipeline_elements, apply_source_settings)
            .start(startup.metrics, startup.units);

        log::info!("🔥 ALUMET measurement pipeline has started.");
        (self.settings.f_after_operation_begin)(&mut pipeline);

        pipeline
    }
}

/// Prints some statistics after the plugin start-up phase.
fn print_stats(startup: &PluginStartup, plugins: &[Box<dyn Plugin>]) {
    // plugins
    let plugins_list = plugins
        .iter()
        .map(|p| format!("    - {} v{}", p.name(), p.version()))
        .collect::<Vec<_>>()
        .join("\n");

    let metrics_list = startup
        .metrics
        .iter()
        .map(|m| format!("    - {}: {} ({})", m.name, m.value_type, m.unit))
        .collect::<Vec<_>>()
        .join("\n");

    let n_sources = startup.pipeline_elements.source_count();
    let n_transforms = startup.pipeline_elements.transform_count();
    let n_output = startup.pipeline_elements.output_count();
    let str_source = if n_sources > 1 { "sources" } else { "source" };
    let str_transform = if n_sources > 1 { "transforms" } else { "transform" };
    let str_output = if n_sources > 1 { "outputs" } else { "output" };
    let pipeline_elements = format!(
        "📥 {} {str_source}, 🔀 {} {str_transform} and 📝 {} {str_output} registered.",
        n_sources, n_transforms, n_output,
    );

    let n_plugins = plugins.len();
    let n_metrics = startup.metrics.len();
    let str_plugin = if n_plugins > 1 { "plugins" } else { "plugin" };
    let str_metric = if n_metrics > 1 { "metrics" } else { "metric" };
    log::info!("Plugin startup complete.\n🧩 {n_plugins} {str_plugin} started:\n{plugins_list}\n📏 {n_metrics} {str_metric} registered:\n{metrics_list}\n{pipeline_elements}");
}

fn apply_source_settings(
    source: Box<dyn pipeline::Source>,
    plugin_name: String,
) -> pipeline::runtime::ConfiguredSource {
    // TODO this should be fetched from the config
    let source_type = pipeline::runtime::SourceType::Normal;
    let trigger_provider = pipeline::trigger::TriggerProvider::TimeInterval {
        start_time: std::time::Instant::now(),
        poll_interval: std::time::Duration::from_secs(1),
        flush_interval: std::time::Duration::from_secs(1),
    };
    pipeline::runtime::ConfiguredSource {
        source,
        plugin_name,
        source_type,
        trigger_provider,
    }
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
    pub fn after_plugin_init(&mut self, f: fn(&mut Vec<Box<dyn Plugin>>)) {
        self.f_after_plugin_init = f;
    }

    /// Defines a function to run after the plugin start-up phase.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_plugin_start(&mut self, f: fn(&mut PluginStartup)) {
        self.f_after_plugin_start = f;
    }

    /// Defines a function to run just after the measurement pipeline has started.
    ///
    /// If a function has already been defined, it is replaced.
    pub fn after_operation_begin(&mut self, f: fn(&mut RunningPipeline)) {
        self.f_after_operation_begin = f;
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
