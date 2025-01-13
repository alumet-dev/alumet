//! Helpers for creating a measurement agent.
//!
//! # Example
//!
//! ```
//! use alumet::{agent, pipeline};
//! use std::time::Duration;
//!
//! # fn f() -> anyhow::Result<()> {
//! let mut pipeline_builder = pipeline::Builder::new();
//! let mut agent_builder = agent::Builder::new(pipeline_builder);
//! // TODO configure the agent, add the plugins, etc.
//!
//! // start Alumet
//! let agent = agent_builder.build_and_start()?;
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

use std::collections::BTreeMap;
use std::{collections::HashMap, ops::DerefMut, time::Duration};

use anyhow::{anyhow, Context};
use indoc::formatdoc;

use crate::plugin::phases::PreStartAction;
use crate::plugin::{AlumetPluginStart, AlumetPostStart, ConfigTable, Plugin, PluginMetadata};
use crate::{
    pipeline::{self, PluginName},
    plugin::{phases::PostStartAction, AlumetPreStart},
};

/// An Agent that has been started.
pub struct RunningAgent {
    pub pipeline: pipeline::MeasurementPipeline,
    pub initialized_plugins: Vec<Box<dyn Plugin>>,
}

/// Agent builder.
///
/// # Example
/// ```no_run
/// use alumet::{agent, pipeline, static_plugins};
///
/// struct MyPlugin {}
/// impl alumet::plugin::rust::AlumetPlugin for MyPlugin {
///     // TODO
/// #   fn name() -> &'static str { "" }
/// #   fn version() -> &'static str { "" }
/// #   fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> { todo!() }
/// #   fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> { todo!() }
/// #   fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> { todo!() }
/// #   fn stop(&mut self) -> anyhow::Result<()> { todo!() }
/// }
///
/// // Get the plugins metadata and configs
/// let plugins = static_plugins![MyPlugin];
/// let mut my_plugin_config: toml::Table = todo!();
///
/// // Create and configure the builder
/// let mut pb = pipeline::Builder::new();
/// let mut builder = agent::Builder::new(pb);
/// builder.add_plugins(plugins);
/// builder.set_plugin_info("my-plugin", true, my_plugin_config);
///
/// // Start Alumet and the plugins
/// let agent = builder.build_and_start();
/// ```
pub struct Builder {
    /// All the plugins (not initialized yet), in order (the order must be preserved).
    plugins: BTreeMap<String, UnitializedPlugin>,

    /// Builds the measurement pipeline.
    pipeline_builder: pipeline::Builder,

    /// Functions called during the agent startup.
    callbacks: Callbacks,
}

struct UnitializedPlugin {
    metadata: PluginMetadata,
    enabled: bool,
    config: toml::Table,
}

struct Callbacks {
    after_plugins_init: Box<dyn FnOnce(&mut Vec<Box<dyn Plugin>>)>,
    after_plugins_start: Box<dyn FnOnce(&pipeline::Builder)>,
    before_operation_begin: Box<dyn FnOnce(&pipeline::Builder)>,
    after_operation_begin: Box<dyn FnOnce(&mut pipeline::MeasurementPipeline)>,
}

impl Default for Callbacks {
    fn default() -> Self {
        Self {
            after_plugins_init: Box::new(|_| ()),
            after_plugins_start: Box::new(|_| ()),
            before_operation_begin: Box::new(|_| ()),
            after_operation_begin: Box::new(|_| ()),
        }
    }
}

impl Builder {
    pub fn new(pipeline_builder: pipeline::Builder) -> Self {
        Self {
            plugins: BTreeMap::new(),
            pipeline_builder,
            callbacks: Callbacks::default(),
        }
    }

    pub fn enabled_disabled_plugins(&self) -> (Vec<&PluginMetadata>, Vec<&PluginMetadata>) {
        let mut enabled = Vec::new();
        let mut disabled = Vec::new();
        for p in self.plugins.values() {
            if p.enabled {
                enabled.push(&p.metadata);
            } else {
                disabled.push(&p.metadata);
            }
        }
        (enabled, disabled)
    }

    pub fn get_plugin(&self, plugin_name: &str) -> Option<(bool, &PluginMetadata)> {
        self.plugins.get(plugin_name).map(|p| (p.enabled, &p.metadata))
    }

    pub fn is_plugin_enabled(&self, plugin_name: &str) -> bool {
        self.plugins.get(plugin_name).map(|p| p.enabled).unwrap_or(false)
    }

    pub fn add_plugin(&mut self, plugin: PluginMetadata) -> &mut Self {
        self.add_plugin_with_info(plugin, true, toml::Table::new())
    }

    pub fn add_plugin_with_info(&mut self, plugin: PluginMetadata, enabled: bool, config: toml::Table) -> &mut Self {
        self.plugins.insert(
            plugin.name.clone(),
            UnitializedPlugin {
                metadata: plugin,
                enabled,
                config,
            },
        );
        self
    }

    pub fn add_plugins(&mut self, plugins: Vec<PluginMetadata>) -> &mut Self {
        self.plugins.extend(plugins.into_iter().map(|meta| {
            (
                meta.name.clone(),
                UnitializedPlugin {
                    metadata: meta,
                    enabled: true,
                    config: toml::Table::new(),
                },
            )
        }));
        self
    }

    pub fn enable_plugin(&mut self, plugin_name: &str) -> &mut Self {
        if let Some(plugin) = self.plugins.get_mut(plugin_name) {
            plugin.enabled = true;
        }
        self
    }

    pub fn disable_plugin(&mut self, plugin_name: &str) -> &mut Self {
        if let Some(plugin) = self.plugins.get_mut(plugin_name) {
            plugin.enabled = false;
        }
        self
    }

    pub fn set_plugin_info(&mut self, plugin_name: &str, enabled: bool, config: toml::Table) -> &mut Self {
        if let Some(plugin) = self.plugins.get_mut(plugin_name) {
            plugin.enabled = enabled;
            plugin.config = config;
        }
        self
    }

    pub fn plugin_config_mut(&mut self, plugin_name: &str) -> Option<&mut toml::Table> {
        self.plugins.get_mut(plugin_name).map(|p| &mut p.config)
    }

    pub fn after_plugins_init<F: FnOnce(&mut Vec<Box<dyn Plugin>>) + 'static>(&mut self, f: F) -> &mut Self {
        self.callbacks.after_plugins_init = Box::new(f);
        self
    }

    pub fn after_plugins_start<F: FnOnce(&pipeline::Builder) + 'static>(&mut self, f: F) -> &mut Self {
        self.callbacks.after_plugins_start = Box::new(f);
        self
    }

    pub fn before_operation_begin<F: FnOnce(&pipeline::Builder) + 'static>(&mut self, f: F) -> &mut Self {
        self.callbacks.before_operation_begin = Box::new(f);
        self
    }

    pub fn after_operation_begin<F: FnOnce(&mut pipeline::MeasurementPipeline) + 'static>(
        &mut self,
        f: F,
    ) -> &mut Self {
        self.callbacks.after_operation_begin = Box::new(f);
        self
    }

    pub fn build_and_start(self) -> anyhow::Result<RunningAgent> {
        /// Initialized one plugin.
        ///
        /// Returns the initialized plugin, or an error.
        fn init_plugin(p: UnitializedPlugin) -> anyhow::Result<Box<dyn Plugin>> {
            let name = p.metadata.name;
            let version = p.metadata.version;
            let config = ConfigTable(p.config);
            log::debug!("Initializing plugin {name} v{version} with config {config:?}...");

            // call init
            let initialized = (p.metadata.init)(config)
                .with_context(|| format!("plugin failed to initialize: {} v{}", name, version))?;

            // check that the plugin corresponds to its metadata
            if (initialized.name(), initialized.version()) != (&name, &version) {
                return Err(anyhow!("invalid plugin: metadata is '{name}' v{version} but the plugin's methods return '{name}' v{version}"));
            }
            Ok(initialized)
        }

        /// Starts a plugin, i.e. calls [`Plugin::start`] with the right context.
        fn start_plugin(
            p: &mut dyn Plugin,
            pipeline_builder: &mut pipeline::Builder,
            pre_start_actions: &mut Vec<(pipeline::PluginName, Box<dyn PreStartAction>)>,
            post_start_actions: &mut Vec<(pipeline::PluginName, Box<dyn PostStartAction>)>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Starting plugin {name} v{version}...");

            let mut ctx = AlumetPluginStart {
                current_plugin: pipeline::PluginName(name.clone()),
                pipeline_builder,
                pre_start_actions,
                post_start_actions,
            };
            p.start(&mut ctx)
                .with_context(|| format!("plugin failed to start: {name} v{version}"))
        }

        /// Executes the pre-pipeline-start phase of a plugin, i.e. calls [`Plugin::pre_pipeline_start`] with the right context.
        fn pre_pipeline_start(
            p: &mut dyn Plugin,
            pipeline_builder: &mut pipeline::Builder,
            actions: &mut HashMap<PluginName, Vec<Box<dyn PreStartAction>>>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Running pre-pipeline-start hook for plugin {name} v{version}...");

            // Prepare the context.
            let pname = pipeline::PluginName(name.clone());
            let mut ctx = AlumetPreStart {
                current_plugin: pname.clone(),
                pipeline_builder,
            };

            // Call pre_pipeline_start.
            p.pre_pipeline_start(&mut ctx)
                .with_context(|| format!("plugin pre_pipeline_start failed: {} v{}", p.name(), p.version()))?;

            // Run the additional actions registered by the plugin, if any.
            if let Some(actions) = actions.remove(&pname) {
                for f in actions {
                    (f)(&mut ctx)
                        .with_context(|| format!("plugin post-pipeline-start action failed: {name} v{version}"))?;
                }
            }
            Ok(())
        }

        /// Executes the post-pipeline-start phase of a plugin, i.e. calls [`Plugin::post_pipeline_start`] with the right context.
        ///
        /// Plugins can also register post-pipeline-start actions in the form of closures, we run these too.
        fn post_pipeline_start(
            p: &mut dyn Plugin,
            pipeline: &mut pipeline::MeasurementPipeline,
            actions: &mut HashMap<PluginName, Vec<Box<dyn PostStartAction>>>,
        ) -> anyhow::Result<()> {
            let name = p.name().to_owned();
            let version = p.version().to_owned();
            log::debug!("Running post-pipeline-start hook for plugin {name} v{version}...");

            // Prepare the context.
            let pname = pipeline::PluginName(name.clone());
            let mut ctx = AlumetPostStart {
                current_plugin: pname.clone(),
                pipeline,
            };

            // Call post_pipeline_start.
            p.post_pipeline_start(&mut ctx)
                .with_context(|| format!("plugin post_pipeline_start method failed: {name} v{version}"))?;

            // Run the additional actions registered by the plugin, if any.
            if let Some(actions) = actions.remove(&pname) {
                for f in actions {
                    (f)(&mut ctx)
                        .with_context(|| format!("plugin post-pipeline-start action failed: {name} v{version}"))?;
                }
            }
            Ok(())
        }

        /// Groups all pre or post-start actions by plugin.
        fn group_plugin_actions<BoxedAction>(
            post_start_actions: Vec<(PluginName, BoxedAction)>,
            n_plugins: usize,
        ) -> HashMap<PluginName, Vec<BoxedAction>> {
            let mut res = HashMap::with_capacity(n_plugins);
            for (plugin, action) in post_start_actions {
                let plugin_actions: &mut Vec<_> = res.entry(plugin).or_default();
                plugin_actions.push(action);
            }
            res
        }

        // Find which plugins are enabled.
        log::info!("Initializing the plugins...");
        let (enabled_plugins, disabled_plugins): (Vec<UnitializedPlugin>, Vec<UnitializedPlugin>) =
            self.plugins.into_values().partition(|p| p.enabled);

        // Initialize the plugins that are enabled.
        let initialized_plugins: anyhow::Result<Vec<Box<dyn Plugin>>> =
            enabled_plugins.into_iter().map(init_plugin).collect();
        let mut initialized_plugins = initialized_plugins?;
        let n_plugins = initialized_plugins.len();
        match n_plugins {
            0 if disabled_plugins.is_empty() => log::warn!("No plugin has been initialized, there may be a problem with your agent implementation. Please check your builder."),
            0 => log::warn!("No plugin has been initialized because they were all disabled in the config. Please check your configuration."),
            1 => log::info!("1 plugin initialized."),
            n => log::info!("{n} plugins initialized."),
        };
        (self.callbacks.after_plugins_init)(&mut initialized_plugins);

        // Start-up phase.
        log::info!("Starting the plugins...");
        let mut pipeline_builder = self.pipeline_builder;
        let mut pre_start_actions = Vec::new();
        let mut post_start_actions = Vec::new();
        for plugin in initialized_plugins.iter_mut() {
            start_plugin(
                plugin.deref_mut(),
                &mut pipeline_builder,
                &mut pre_start_actions,
                &mut post_start_actions,
            )?;
        }
        print_stats(&pipeline_builder, &initialized_plugins, &disabled_plugins);
        (self.callbacks.after_plugins_start)(&pipeline_builder);

        // pre-pipeline-start actions
        log::info!("Running pre-pipeline-start hooks...");
        let mut pre_actions_per_plugin = group_plugin_actions(pre_start_actions, n_plugins);
        for plugin in initialized_plugins.iter_mut() {
            pre_pipeline_start(plugin.deref_mut(), &mut pipeline_builder, &mut pre_actions_per_plugin)?;
        }
        (self.callbacks.before_operation_begin)(&pipeline_builder);

        // Build and start the pipeline.
        log::info!("Starting the measurement pipeline...");
        let mut pipeline = pipeline_builder.build().context("Pipeline failed to build")?;
        log::info!("🔥 ALUMET measurement pipeline has started.");

        // post-pipeline-start actions
        log::info!("Running post-pipeline-start hooks...");
        let mut post_actions_per_plugin = group_plugin_actions(post_start_actions, n_plugins);
        for plugin in initialized_plugins.iter_mut() {
            post_pipeline_start(plugin.deref_mut(), &mut pipeline, &mut post_actions_per_plugin)?;
        }
        (self.callbacks.after_operation_begin)(&mut pipeline);

        log::info!("🔥 ALUMET agent is ready.");

        let agent = RunningAgent {
            pipeline,
            initialized_plugins,
        };
        Ok(agent)
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

/// Utilities for agent configurations.
pub mod config {
    use std::{borrow::Cow, collections::HashMap, env::VarError, path::Path, str::FromStr};

    use anyhow::{anyhow, Context};
    use fancy_regex::{Captures, Regex};

    use crate::plugin::PluginMetadata;

    /// ENV_VAR is a regex which matches every unescaped linux environment
    /// i.e. `$VAR` or `$VAR_BIS` for example but not `\$ESCAPED_VAR`
    const ENV_VAR: &'static str = r"\w?(?<!\\)\$([[:word:]]*)";

    /// Extracts the configuration of each plugin from the `config`.
    ///
    /// If a plugin's config contains the key `enabled`, it is used to determine whether
    /// the plugin is enabled or disabled. If there is no such key, the plugin is enabled.
    ///
    /// # Example
    ///
    /// Configuration before:
    /// ```toml
    /// app_param = "v"
    ///
    /// [plugins.a]
    /// count = 123
    ///
    /// [plugins.b]
    /// enabled = false
    /// ```
    ///
    /// Configuration after:
    /// ```toml
    /// app_param = "v"
    /// ```
    ///
    /// And the result contains:
    /// ```toml
    /// a -> (true, {count = 123})
    /// b -> (false, {})
    /// ```
    pub fn extract_plugin_configs(config: &mut toml::Table) -> anyhow::Result<HashMap<String, (bool, toml::Table)>> {
        let plugins_table = match config.remove("plugins") {
            Some(toml::Value::Table(t)) => Ok(t),
            Some(bad_value) => Err(anyhow!(
                "invalid global config: 'plugins' must be a table, not a {}.",
                bad_value.type_str()
            )),
            None => Err(anyhow!("invalid global config: it should contain a 'plugins' table")),
        }?;
        let mut res = HashMap::with_capacity(plugins_table.len());
        for (plugin, config) in plugins_table {
            let (enabled, config): (bool, toml::Table) = match config {
                toml::Value::Table(mut t) => match t.remove("enable") {
                    Some(toml::Value::Boolean(b)) => (b, t),
                    Some(bad_value) => {
                        return Err(anyhow!(
                            "invalid value in plugin config: 'plugins.{plugin}.enabled' must be a boolean, not a {}.",
                            bad_value.type_str()
                        ));
                    }
                    None => (true, t),
                },
                bad_value => {
                    return Err(anyhow!(
                        "invalid plugin config: 'plugins.{plugin}' must be a table, not a {}.",
                        bad_value.type_str()
                    ))
                }
            };
            res.insert(plugin, (enabled, config));
        }
        Ok(res)
    }

    /// Generates the default configuration of each plugin in the slice, and insert them into
    /// a sub-table of `config` named `plugins`.
    ///
    /// If `config` does not contain a `plugins` entry, it is created.
    /// If `config` does contain a `plugins` entry, but it is not a table, returns an error.
    pub fn insert_default_plugin_configs(plugins: &[PluginMetadata], config: &mut toml::Table) -> anyhow::Result<()> {
        let plugins_table: &mut toml::Table = config
            .entry(String::from("plugins"))
            .or_insert_with(|| toml::Value::Table(toml::Table::with_capacity(plugins.len())))
            .as_table_mut()
            .context("value 'plugins' should be a TOML table")?;

        for plugin in plugins {
            log::debug!("Generating default config for plugin {}", plugin.name);
            let plugin_config = (plugin.default_config)()?;

            log::debug!("default config: {plugin_config:?}");
            let default_plugin_config = plugin_config.map(|conf| conf.0).unwrap_or_else(|| toml::Table::new());
            plugins_table.insert(plugin.name.to_owned(), toml::Value::Table(default_plugin_config));
        }
        Ok(())
    }

    /// Parses a TOML configuration file and applies environment variable substitution.
    ///
    /// # Default config
    /// If the file does not exist, the `default` closure is called
    /// to generate a default configuration.
    ///
    /// This new configuration is saved to the file, then returned.
    pub fn parse_file_with_default<F: FnOnce() -> anyhow::Result<toml::Table>>(
        config_path: &Path,
        default: F,
    ) -> anyhow::Result<toml::Table> {
        parse_file(config_path, Some(default))
    }

    /// Parses a TOML configuration file and applies environment variable substitution.
    ///
    /// # Default config
    /// If the file does not exist, the `default` closure is called
    /// to generate a default configuration.
    ///
    /// This new configuration is saved to the file, then returned.
    fn parse_file<F: FnOnce() -> anyhow::Result<toml::Table>>(
        config_path: &Path,
        default: Option<F>,
    ) -> anyhow::Result<toml::Table> {
        match std::fs::read_to_string(config_path) {
            Ok(file_content) => parse_str(&file_content),
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::NotFound => {
                        if let Some(provide_default) = default {
                            // generate the default config
                            let default_config: toml::Table = provide_default()
                                .with_context(|| format!("failed to generate a default config for {config_path:?}"))?;

                            // write the config to the file
                            std::fs::write(config_path, default_config.to_string())
                                .with_context(|| format!("failed to write the default config to {config_path:?}"))?;

                            // return it
                            log::info!("Default configuration written to {}", config_path.display());
                            Ok(default_config)
                        } else {
                            Err(anyhow!(
                                "configuration file not found (and no default specified): {config_path:?}"
                            ))
                        }
                    }
                    _ => Err(anyhow!("failed to load config from {config_path:?}: {e}",)),
                }
            }
        }
    }

    /// Parses a TOML configuration and applies environment variable substitution.
    pub fn parse_str(config_content: &str) -> anyhow::Result<toml::Table> {
        // Replace the environment variables.
        // `replace_all` is not designed for fallible replacement, so we accumulate errors in vectors.
        let regex = Regex::new(ENV_VAR).expect("the ENV_VAR regex should be valid");
        let mut missing_env_vars = Vec::new();
        let mut invalid_env_vars = Vec::new();
        let config_content: Cow<str> = regex.replace_all(config_content, |c: &Captures| match &c[1] {
            // If the captured string is `$` then we manage it as an escaped `$`.
            "" => String::from("$"),
            // Else, we replace the `$varname` by its value, handling errors.
            key => std::env::var(key)
                .inspect_err(|e| match e {
                    VarError::NotPresent => missing_env_vars.push(key.to_owned()),
                    VarError::NotUnicode(_) => invalid_env_vars.push(key.to_owned()),
                })
                .unwrap_or_default(),
        });
        if !missing_env_vars.is_empty() {
            return Err(anyhow!(
                "missing environment variables: {}",
                missing_env_vars.join(", ")
            ));
        }
        if !invalid_env_vars.is_empty() {
            return Err(anyhow!(
                "invalid environment variables (not valid UTF-8): {}",
                invalid_env_vars.join(", ")
            ));
        }
        toml::Table::from_str(&config_content).context("invalid TOML configuration")
    }

    #[cfg(test)]
    mod tests {
        use fancy_regex::Regex;

        use crate::agent::config::ENV_VAR;

        #[test]
        fn regex() {
            let compiled_env_var = Regex::new(ENV_VAR).expect("the ENV_VAR regex should be valid");
            // Verifying that the ENV_VAR regex is still working.
            let mut result = compiled_env_var.find("abc $VAR abc");

            assert!(result.is_ok(), "execution wasn't successful");
            let mut match_option = result.unwrap();

            assert!(match_option.is_some(), "did not found a match");
            let mut m = match_option.unwrap();

            assert_eq!(m.as_str(), "$VAR");

            result = compiled_env_var.find("abc $VAR_BIS abc");

            assert!(result.is_ok(), "execution wasn't successful");
            match_option = result.unwrap();

            assert!(match_option.is_some(), "did not found a match");
            m = match_option.unwrap();

            assert_eq!(m.as_str(), "$VAR_BIS");

            result = compiled_env_var.find(r"abc \$VAR_BIS abc");

            assert!(result.is_ok(), "execution wasn't successful");
            match_option = result.unwrap();

            assert!(
                match_option.is_none(),
                "The regex didn't ignored the escaped environment variable"
            );
        }
    }
}

/// Prints some statistics after the plugin start-up phase.
fn print_stats(
    pipeline_builder: &pipeline::Builder,
    enabled_plugins: &[Box<dyn Plugin>],
    disabled_plugins: &[UnitializedPlugin],
) {
    macro_rules! pluralize {
        ($count:expr, $str:expr) => {
            if $count > 1 {
                concat!($str, "s")
            } else {
                $str
            }
        };
    }

    // format plugin lists
    let enabled_list: String = enabled_plugins
        .iter()
        .map(|p| format!("    - {} v{}", p.name(), p.version()))
        .collect::<Vec<_>>()
        .join("\n");
    let disabled_list: String = disabled_plugins
        .iter()
        .map(|p| format!("    - {} v {}", p.metadata.name, p.metadata.version))
        .collect::<Vec<_>>()
        .join("\n");
    let n_enabled = enabled_plugins.len();
    let n_disabled = disabled_plugins.len();
    let enabled_str = pluralize!(n_enabled, "plugin");
    let disabled_str = pluralize!(n_disabled, "plugin");

    // format metric list
    let metrics = &pipeline_builder.metrics;
    let metric_list = if metrics.is_empty() {
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

    // format pipeline statistics
    let stats = pipeline_builder.stats();

    let n_sources = stats.sources;
    let n_transforms = stats.transforms;
    let n_outputs = stats.outputs;
    let n_metric_listeners = stats.metric_listeners;

    let source_str = pluralize!(n_sources, "source");
    let transform_str = pluralize!(n_transforms, "transform");
    let output_str = pluralize!(n_outputs, "output");
    let metric_listener_str = pluralize!(n_metric_listeners, "metric listener");

    let n_metrics = stats.metrics;
    let str_metric = pluralize!(n_metrics, "metric");
    let msg = formatdoc! {"
        Plugin startup complete.
        🧩 {n_enabled} {enabled_str} started:
        {enabled_list}
        
        ⭕ {n_disabled} {disabled_str} disabled:
        {disabled_list}
        
        📏 {n_metrics} {str_metric} registered:
        {metric_list}
        
        📥 {n_sources} {source_str}, 🔀 {n_transforms} {transform_str} and 📝 {n_outputs} {output_str} registered.
        
        🔔 {n_metric_listeners} {metric_listener_str} registered.
        "
    };
    log::info!("{msg}");
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

pub use static_plugins;

#[cfg(test)]
mod tests {
    use std::env;

    use serde::Serialize;

    use crate::plugin::rust::{serialize_config, AlumetPlugin};
    use crate::plugin::{AlumetPluginStart, ConfigTable};

    use super::config::extract_plugin_configs;

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

    #[test]
    fn parse_config_string_basic() {
        let config_content = r#"
            key = "value"
            
            [plugins.name]
            list = ["a", "b"]
            count = 1
        "#;
        let config = super::config::parse_str(&config_content).unwrap();
        // Assert that we get the same thing as toml::Table::from_str, because
        // we don't use any environment variable here.
        assert_eq!(
            config,
            config_content.parse::<toml::Table>().unwrap(),
            "returned config is wrong"
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

        let config = super::config::parse_file_with_default(&config_path, || {
            panic!("default config provider should not be called")
        })
        .unwrap();

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
    fn parse_config_file_with_missing_env_var() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("test-config-with-wrong-env-var.toml");
        let config_content = r#"
            key = "value"
            
            [plugins.name]
            list = ["a", "b"]
            url = "$URL_BIS"
        "#;
        std::fs::write(&config_path, config_content).unwrap();

        // URL_BIS doesn't exist, parsing should return an error.
        super::config::parse_file_with_default(&config_path, || panic!("default config provider should not be called"))
            .expect_err("should fail");
    }

    #[test]
    fn create_default_config_file() {
        let tmp = std::env::temp_dir();
        let config_path = tmp.join("I-do-not-exist.toml");
        let _ = std::fs::remove_file(&config_path);

        let plugins = static_plugins![MyPlugin];
        let make_default = || {
            let mut config = toml::Table::new();
            super::config::insert_default_plugin_configs(&plugins, &mut config)?;
            Ok(config)
        };
        let config = super::config::parse_file_with_default(&config_path, make_default).unwrap();
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

    #[test]
    fn enable_flag() -> anyhow::Result<()> {
        let mut config: toml::Table = r#"
            [plugins.a]
            list = ["default-item"]
            count = 42
            
            [plugins.b]
            enable = false
            key = "value"
            
            [plugins.c]
            count = 0
            enable = true
        "#
        .parse()?;

        let config_a: toml::Table = r#"
            list = ["default-item"]
            count = 42
        "#
        .parse()?;
        let config_b: toml::Table = r#"
            key = "value"
        "#
        .parse()?;
        let config_c: toml::Table = r#"
            count = 0
        "#
        .parse()?;

        let extracted = extract_plugin_configs(&mut config)?;
        assert!(extracted.get("a").unwrap().0); // a enabled
        assert!(!extracted.get("b").unwrap().0); // b disabled
        assert!(extracted.get("c").unwrap().0); // c enabled

        assert_eq!(extracted.get("a").unwrap().1, config_a);
        assert_eq!(extracted.get("b").unwrap().1, config_b);
        assert_eq!(extracted.get("c").unwrap().1, config_c);
        Ok(())
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
