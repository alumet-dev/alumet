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

pub enum PluginStatus {
    Enabled,
    Disabled,
    Any,
}

pub enum UnknownPluginInConfigPolicy {
    LogWarn,
    LogWarnIfEnabled,
    Error,
    ErrorIfEnabled,
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

impl PluginSet {
    pub fn new(metadata: Vec<PluginMetadata>) -> Self {
        let map = BTreeMap::from_iter(metadata.into_iter().map(|m| (m.name.clone(), PluginInfo::new(m))));
        Self(map)
    }

    /// Enables the specified plugins and disables all the others.
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

    pub fn extract_config(
        &mut self,
        global_config: &mut toml::Table,
        allow_status_change: bool,
        on_unknown: UnknownPluginInConfigPolicy,
    ) -> anyhow::Result<()> {
        let extracted = super::config::extract_plugins_config(global_config).context("invalid config")?;
        for (plugin_name, (enabled, config)) in extracted {
            if let Some(plugin_info) = self.0.get_mut(&plugin_name) {
                if allow_status_change {
                    plugin_info.enabled = enabled;
                }
                plugin_info.config = Some(config);
            } else {
                match on_unknown {
                    UnknownPluginInConfigPolicy::LogWarn => {
                        log::warn!("unknown plugin '{plugin_name}' in configuration")
                    }
                    UnknownPluginInConfigPolicy::LogWarnIfEnabled => {
                        if enabled {
                            log::warn!("unknown plugin '{plugin_name}' in configuration")
                        }
                    }
                    UnknownPluginInConfigPolicy::Error => {
                        return Err(anyhow!("unknown plugin '{plugin_name}' in configuration"))
                    }
                    UnknownPluginInConfigPolicy::ErrorIfEnabled => {
                        if enabled {
                            return Err(anyhow!("unknown plugin '{plugin_name}' in configuration"));
                        }
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
    pub fn metadata(&self, filter: PluginStatus) -> impl Iterator<Item = &PluginMetadata> {
        self.0
            .values()
            .filter_map(move |p| if filter.accept(&p) { Some(&p.metadata) } else { None })
    }

    /// Consumes the set and returns two lists: the enabled plugins,
    /// and the disabled plugins.
    pub fn into_partition(self) -> (Vec<PluginInfo>, Vec<PluginInfo>) {
        // (enabled, disabled)
        self.0.into_values().partition(|p| p.enabled)
    }

    /// Collects the plugins to a `Vec<PluginMetadata>`, filtered by status.
    pub fn into_metadata(self, filter: PluginStatus) -> Vec<PluginMetadata> {
        self.0
            .into_values()
            .filter(|p| filter.accept(p))
            .map(|p| p.metadata)
            .collect()
    }
}

impl PluginStatus {
    fn accept(&self, p: &PluginInfo) -> bool {
        match self {
            PluginStatus::Enabled => p.enabled,
            PluginStatus::Disabled => !p.enabled,
            PluginStatus::Any => true,
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
