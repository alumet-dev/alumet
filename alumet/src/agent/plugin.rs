//! Building a set of plugins with their configuration options.

use std::collections::BTreeMap;

use anyhow::{anyhow, Context};

use crate::plugin::PluginMetadata;

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
///
/// Attributes are supported:
/// ```ignore
/// use alumet::plugin::PluginMetadata;
///
/// let plugins = static_plugins![
///     #[cfg(feature = "some-feature")]
///     ConditionalPlugin
/// ];
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
    [$( $(#[$m:meta])* $x:path ),+ $(,)?] => {
    //  ^^^^^^^^^^^^^^ accepts zero or more #[attribute]
        {
            vec![
                $(
                    $(#[$m])* // expands the attributes, if any
                    $crate::plugin::PluginMetadata::from_static::<$x>(),
                )*
            ] as Vec<$crate::plugin::PluginMetadata>
        }
    }
}

/// Information about a plugin that has not been created yet
/// (i.e. [`PluginMetadata::init`] has not been called).
pub struct PluginInfo {
    pub metadata: PluginMetadata,
    pub enabled: bool,
    pub config: Option<toml::Table>,
}

/// A set of non-created plugins, with their metadata and configuration.
///
/// The order of the plugins is preserved: they are stored in the same order as they are added to the set.
pub struct PluginSet(BTreeMap<String, PluginInfo>);

/// Filters plugins based on their status.
pub enum PluginFilter {
    /// Matches enabled plugins.
    Enabled,
    /// Matches disabled plugins.
    Disabled,
    /// Matches all plugins.
    Any,
}

/// How to react when the config contains an unknown plugin.
pub enum UnknownPluginInConfigPolicy {
    /// Logs a warning message and continues.
    LogWarn,
    /// Returns an error.
    Error,
    /// Ignores the plugin config and continues.
    Ignore,
}

impl PluginInfo {
    fn new(metadata: PluginMetadata) -> Self {
        Self {
            metadata,
            enabled: true,
            config: None,
        }
    }
}

impl From<Vec<PluginMetadata>> for PluginSet {
    /// Creates a new plugin set from their metadata.
    ///
    /// Every plugin is marked as enabled. No configuration is attached to the plugins.
    fn from(metadata: Vec<PluginMetadata>) -> Self {
        let map = BTreeMap::from_iter(metadata.into_iter().map(|m| (m.name.clone(), PluginInfo::new(m))));
        Self(map)
    }
}

impl PluginSet {
    /// Creates a new empty plugin set.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Enables the specified plugins and disables all the others.
    ///
    /// Plugins that are not in the set are ignored.
    pub fn enable_only(&mut self, plugin_names: &[impl AsRef<str>]) {
        // We disable every plugin and re-enable only the ones we are interested in.
        // Cost: O(P+E) where P is the number of plugins in the set and E the size of `plugin_names`,
        // assuming that the cost of one lookup in the set is 1.
        for p in self.0.values_mut() {
            p.enabled = false;
        }
        for p in plugin_names {
            self.set_plugin_enabled(p.as_ref(), true);
        }
    }

    /// Extracts the config of each plugin.
    ///
    /// Use `on_unknown` to choose what to do when the config mentions a plugin that is not in the plugin set.
    ///
    /// # Enabling/disabling plugins
    /// If `update_status` is true, enable/disable the plugins according to the configuration.
    /// Plugins that are present in the config are enabled, those that are not present are disabled.
    /// If the configuration of a plugin contains an `enabled` or `enable` boolean key, its value determines
    /// whether the plugin is enabled or not.
    pub fn extract_config(
        &mut self,
        global_config: &mut toml::Table,
        update_status: bool,
        on_unknown: UnknownPluginInConfigPolicy,
    ) -> anyhow::Result<()> {
        // Disable every plugin first.
        if update_status {
            for plugin_info in self.0.values_mut() {
                plugin_info.enabled = false;
            }
        }

        // Extract the config and enable the plugins that it contains.
        let extracted = super::config::extract_plugins_config(global_config).context("invalid config")?;
        for (plugin_name, (enabled, config)) in extracted {
            if let Some(plugin_info) = self.0.get_mut(&plugin_name) {
                if update_status {
                    plugin_info.enabled = enabled;
                }
                plugin_info.config = Some(config);
            } else {
                match on_unknown {
                    UnknownPluginInConfigPolicy::LogWarn => {
                        log::warn!("unknown plugin '{plugin_name}' in configuration")
                    }
                    UnknownPluginInConfigPolicy::Error => {
                        return Err(anyhow!("unknown plugin '{plugin_name}' in configuration"))
                    }
                    UnknownPluginInConfigPolicy::Ignore => {
                        // do nothing
                    }
                }
            }
        }
        Ok(())
    }

    /// Gets the information about a non-initialized plugin.
    pub fn get_plugin(&self, plugin_name: &str) -> Option<&PluginInfo> {
        self.0.get(plugin_name)
    }

    /// Gets the information about a non-initialized plugin.
    pub fn get_plugin_mut(&mut self, plugin_name: &str) -> Option<&mut PluginInfo> {
        self.0.get_mut(plugin_name)
    }

    /// Checks if a plugin is enabled.
    ///
    /// If the plugin is not in the set, returns `false`.
    pub fn is_plugin_enabled(&self, plugin_name: &str) -> bool {
        self.0.get(plugin_name).map(|p| p.enabled).unwrap_or(false)
    }

    /// Enables or disables a plugin.
    pub fn set_plugin_enabled(&mut self, plugin_name: &str, enabled: bool) {
        if let Some(plugin) = self.0.get_mut(plugin_name) {
            plugin.enabled = enabled;
        }
    }

    /// Adds a new plugin to the set.
    ///
    /// The plugin is not initialized yet.
    pub fn add_plugin(&mut self, plugin: PluginInfo) {
        self.0.insert(plugin.metadata.name.clone(), plugin);
    }

    /// Adds multiple un-initialized plugins to the set.
    pub fn add_plugins(&mut self, plugins: Vec<PluginInfo>) {
        self.0.extend(plugins.into_iter().map(|p| (p.metadata.name.clone(), p)));
    }

    /// Iterates on the metadata of the plugins that match the given status filter.
    pub fn metadata(&self, filter: PluginFilter) -> impl Iterator<Item = &PluginMetadata> {
        self.0
            .values()
            .filter_map(move |p| if filter.accept(p) { Some(&p.metadata) } else { None })
    }

    /// Consumes the set and returns two lists: the enabled plugins,
    /// and the disabled plugins.
    pub fn into_partition(self) -> (Vec<PluginInfo>, Vec<PluginInfo>) {
        // (enabled, disabled)
        self.0.into_values().partition(|p| p.enabled)
    }

    /// Collects the plugins to a `Vec<PluginMetadata>`, filtered by status.
    pub fn into_metadata(self, filter: PluginFilter) -> Vec<PluginMetadata> {
        self.0
            .into_values()
            .filter(|p| filter.accept(p))
            .map(|p| p.metadata)
            .collect()
    }
}

impl PluginFilter {
    /// Checks if a plugin matches this filter.
    fn accept(&self, p: &PluginInfo) -> bool {
        match self {
            PluginFilter::Enabled => p.enabled,
            PluginFilter::Disabled => !p.enabled,
            PluginFilter::Any => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use crate::plugin::{
        rust::{serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable,
    };

    mod macros {
        use super::MyPlugin;

        #[test]
        fn static_plugins_macro() {
            let a = static_plugins![MyPlugin];
            let b = static_plugins![MyPlugin,];
            let empty = static_plugins![];
            assert_eq!(1, a.len());
            assert_eq!(1, b.len());
            assert_eq!(a[0].name, b[0].name);
            assert_eq!(a[0].version, b[0].version);
            assert!(empty.is_empty());
        }

        #[test]
        fn static_plugins_macro_with_attributes() {
            let single = static_plugins![
                #[cfg(test)]
                MyPlugin,
            ];
            assert_eq!(1, single.len());

            let empty = static_plugins![
                #[cfg(not(test))]
                MyPlugin
            ];
            assert_eq!(0, empty.len());

            let multiple = static_plugins![
                #[cfg(test)]
                MyPlugin,
                #[cfg(not(test))]
                MyPlugin,
                #[cfg(test)]
                MyPlugin
            ];
            assert_eq!(2, multiple.len());
        }
    }

    mod plugin_set {
        use toml::toml;

        use crate::plugin::rust::AlumetPlugin;

        use super::super::{PluginInfo, PluginMetadata, PluginSet, UnknownPluginInConfigPolicy};
        use super::MyPlugin;

        fn plugin_set() -> PluginSet {
            let mut set = PluginSet::new();
            set.add_plugin(PluginInfo {
                metadata: PluginMetadata::from_static::<MyPlugin>(),
                enabled: false,
                config: None,
            });
            assert_eq!(set.0.len(), 1);
            assert!(set.get_plugin(MyPlugin::name()).is_some());
            set
        }

        #[test]
        fn extract_config_no_update() {
            let mut set = plugin_set();
            let mut global_config = toml! {
                global = 0

                [plugins.name]
                n = 123
            };

            // extract with `update_status=false`, the plugin should not be enabled yet.
            set.extract_config(&mut global_config, false, UnknownPluginInConfigPolicy::Error)
                .expect("config should be valid");
            // check that the plugin's config has been moved to its PluginInfo
            let plugin_info = set.get_plugin("name").unwrap();
            assert_eq!(
                plugin_info
                    .config
                    .as_ref()
                    .expect("plugin config should be set")
                    .get("n"),
                Some(&toml::Value::Integer(123))
            );
            // check that the plugin is NOT enabled
            assert!(!plugin_info.enabled);
            assert!(!set.is_plugin_enabled("name"));
            // check that the plugins' configs are no longer in the global config
            assert!(global_config.get("plugins").is_none());
        }

        #[test]
        fn extract_config_update_status_implicitly_enabled() {
            let mut set = plugin_set();
            let mut global_config = toml! {
                global = 0

                [plugins.name]
                n = 123
            };

            // extract with `update_status=true`, the plugin should be enabled
            set.extract_config(&mut global_config, true, UnknownPluginInConfigPolicy::Error)
                .expect("config should be valid");
            let plugin_info = set.get_plugin("name").unwrap();
            assert_eq!(
                plugin_info
                    .config
                    .as_ref()
                    .expect("plugin config should be set")
                    .get("n"),
                Some(&toml::Value::Integer(123))
            );
            assert!(plugin_info.enabled);
            assert!(set.is_plugin_enabled("name"));
        }

        #[test]
        fn extract_config_update_status_implicitly_disabled() {
            let mut set = plugin_set();
            let mut global_config = toml! {
                global = 0
            };

            // extract with `update_status=true`, the plugin should be enabled
            set.extract_config(&mut global_config, true, UnknownPluginInConfigPolicy::Error)
                .expect("config should be valid");
            let plugin_info = set.get_plugin("name").unwrap();
            assert!(plugin_info.config.is_none(), "plugin should have no config");
            assert!(!plugin_info.enabled);
            assert!(!set.is_plugin_enabled("name"));
        }

        #[test]
        fn extract_config_update_status_explicitly_disabled() {
            let mut set = plugin_set();
            let mut global_config = toml! {
                global = 0

                [plugins.name]
                enabled = false
                n = 123
            };

            // extract with `update_status=true`, the plugin should be enabled
            set.extract_config(&mut global_config, true, UnknownPluginInConfigPolicy::Error)
                .expect("config should be valid");
            let plugin_info = set.get_plugin("name").unwrap();
            assert_eq!(
                plugin_info
                    .config
                    .as_ref()
                    .expect("plugin config should be set")
                    .get("n"),
                Some(&toml::Value::Integer(123))
            );
            assert!(!plugin_info.enabled);
            assert!(!set.is_plugin_enabled("name"));
        }

        #[test]
        fn extract_config_update_status_explicitly_enabled() {
            let mut set = plugin_set();
            let mut global_config = toml! {
                global = 0

                [plugins.name]
                enabled = true
                n = 123
            };

            // extract with `update_status=true`, the plugin should be enabled
            set.extract_config(&mut global_config, true, UnknownPluginInConfigPolicy::Error)
                .expect("config should be valid");
            let plugin_info = set.get_plugin("name").unwrap();
            assert_eq!(
                plugin_info
                    .config
                    .as_ref()
                    .expect("plugin config should be set")
                    .get("n"),
                Some(&toml::Value::Integer(123))
            );
            assert!(plugin_info.enabled);
            assert!(set.is_plugin_enabled("name"));
        }
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
