//! Helpers for managing the lifecycle of plugins.

use super::{AlumetStart, Plugin, PluginMetadata};
use crate::{config::ConfigTable, pipeline::builder::PipelineBuilder};

/// Helper for the plugin initialization phase.
pub struct PluginInitialization {
    /// The global configuration of the Alumet agent,
    /// which contains the configuration of each plugin (one table per plugin).
    pub global_config: toml::Table,
}

impl PluginInitialization {
    pub fn new(global_config: toml::Table) -> Self {
        Self { global_config }
    }

    pub fn initialize(&mut self, plugin: PluginMetadata) -> anyhow::Result<Box<dyn Plugin>> {
        let name = &plugin.name;
        let version = &plugin.version;
        let sub_config = self.global_config.remove(name);
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
}

/// Helper for the plugin start-up phase.
///
/// This structure contains everything that is needed to start a
/// list of plugins.
pub struct PluginStartup {
    /// Builder for the measurement pipeline.
    pub pipeline_builder: PipelineBuilder,
}

impl PluginStartup {
    pub fn new() -> Self {
        Self {
            pipeline_builder: PipelineBuilder::new(),
        }
    }

    /// Starts a plugin by calling its [`start`](Plugin::start) method.
    pub fn start(&mut self, plugin: &mut dyn Plugin) -> anyhow::Result<()> {
        let mut start = AlumetStart {
            pipeline_builder: &mut self.pipeline_builder,
            current_plugin_name: plugin.name().to_owned(),
        };
        plugin.start(&mut start)
    }
}
