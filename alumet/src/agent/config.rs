//! Configuration management.
//!
//! # Agent configuration
//!
//! Alumet uses TOML as its default configuration format.
//!
//! The configuration of an agent looks like the following example.
//!  
//! ```toml
//! # general options here
//! option = "something"
//! foo = "bar"
//!
//! # A "plugins" table with one sub-table per plugin
//! [plugins.a]
//! plugin_a_option = "value"
//!
//! [plugins.b]
//! plugin_b_option = 123
//! ```
//!
//! # Loading the configuration
//!
//! Use the [`Loader`] to parse the configuration file with various options.
//!
//! ```rust,no_run
//! use alumet::agent::config;
//! use alumet::agent::plugin::PluginSet;
//!
//! let mut plugins = PluginSet::new();
//! // TODO add your plugins
//!
//! let default_config_provider = config::AutoDefaultConfigProvider::new(
//!     &plugins, // populate the agent config with the default config section of each enabled plugin
//!     || toml::Table::new() // no general options (you can return a struct that implements `Serialize`)
//! );
//!
//! let mut config = config::Loader::parse_file("alumet-config.toml")
//!     .or_default(default_config_provider, true) // if the config file does not exist, generate it
//!     .load() // load now
//!     .expect("could not load config file");
//!
//! // TODO use the config
//! ```
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::{borrow::Cow, env::VarError};

use anyhow::anyhow;
use serde::Serialize;

use super::plugin::{PluginFilter, PluginSet};
use crate::plugin::PluginMetadata;
use error::*;

/// Loads the agent configuration from a TOML file.
pub struct Loader<'d> {
    /// File that contains the configuration.
    file: PathBuf,
    /// Provides the default configuration, in case the file is missing.
    default_provider: Option<Box<dyn DefaultConfigProvider + 'd>>,
    /// Should the default config be saved after generation?
    save_default: bool,
    /// Additional values that override the content of the config.
    overrides: Option<toml::Table>,
    /// Should environment variable substitution be applied before deserializing?
    substitute_env: bool,
}

/// Generates default configurations.
///
/// See [`AutoDefaultConfigProvider`] for the "standard" implementation.
pub trait DefaultConfigProvider {
    /// Gets the default config as a structured TOML table.
    fn default_config(&self) -> anyhow::Result<toml::Table>;

    /// Gets the default config as a string.
    ///
    /// The default implementation serializes the result of [`default_config`].
    fn default_config_string(&self) -> anyhow::Result<String> {
        let config = self.default_config()?;
        let string = toml::to_string_pretty(&toml::Value::Table(config))?;
        Ok(string)
    }
}

/// Generates default configurations by combining two things:
/// - general config options, provided by a function `F`
/// - the default config of every plugin that is enabled in a [`PluginSet`]
///
/// # Config structure
/// The generated configuration follows what is expected by [`PluginSet::extract_config`]
/// and other agent-related functions. Refer to the module documentation for more information.
pub struct AutoDefaultConfigProvider<'p, A: Serialize, F: Fn() -> A> {
    plugins: &'p PluginSet,
    default_general_options: F,
}

/// When asked to generate a default configuration, fails with an error.
pub struct NoDefaultConfigProvider;

impl<'d> Loader<'d> {
    /// Creates a new `Loader` that will read `file_path` on [`load`](Self::load).
    pub fn parse_file<P: Into<PathBuf>>(config_file: P) -> Self {
        Self {
            file: config_file.into(),
            default_provider: None,
            save_default: false,
            overrides: None,
            substitute_env: false,
        }
    }

    /// If the configuration file does not exist, use the `default_provider`.
    ///
    /// Set `save_to_file` to `true` to write the default config to the file specified
    /// by [`parse_file`](Self::parse_file).
    pub fn or_default<D: DefaultConfigProvider + 'd>(mut self, default_provider: D, save_to_file: bool) -> Self {
        self.default_provider = Some(Box::new(default_provider));
        self.save_default = save_to_file;
        self
    }

    /// If the configuration file does not exist, use the `default_provider`.
    ///
    /// Set `save_to_file` to `true` to write the default config to the file specified
    /// by [`parse_file`](Self::parse_file).
    pub fn or_default_boxed(
        mut self,
        default_provider: Box<dyn DefaultConfigProvider + 'd>,
        save_to_file: bool,
    ) -> Self {
        self.default_provider = Some(default_provider);
        self.save_default = save_to_file;
        self
    }

    /// Overrides the content of the configuration by [merging](merge_override) it
    /// with another config.
    ///
    /// Multiple overrides can be set. The overrides are applied in order.
    pub fn with_override(mut self, config_override: toml::Table) -> Self {
        match &mut self.overrides {
            Some(existing) => merge_override(existing, config_override),
            None => self.overrides = Some(config_override),
        }
        self
    }

    /// Enables or disables the substitution of environment variables.
    ///
    /// Variable substitution is performed _before_ passing the content of the config
    /// file to the TOML parser.
    pub fn substitute_env_variables(mut self, substitute_env: bool) -> Self {
        self.substitute_env = substitute_env;
        self
    }

    /// Loads the configuration with the provided settings.
    pub fn load(mut self) -> Result<toml::Table, LoadError> {
        self.load_impl().map_err(|e| LoadError {
            config_file: self.file,
            kind: e,
        })
    }

    fn load_impl(&mut self) -> Result<toml::Table, LoadErrorCause> {
        let config_content = self.read_config_or_default()?;
        let config_content = substitute_env(&config_content)?;
        let mut parsed_config = toml::Table::from_str(&config_content)?;
        if let Some(overrides) = self.overrides.take() {
            merge_override(&mut parsed_config, overrides);
        }
        Ok(parsed_config)
    }

    fn read_config_or_default(&mut self) -> Result<String, LoadErrorCause> {
        match std::fs::read_to_string(&self.file) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // no config file, try the default
                if let Some(default_provider) = self.default_provider.take() {
                    // get the default config
                    let default_content = default_provider
                        .default_config_string()
                        .map_err(LoadErrorCause::DefaultProvider)?;

                    // save the default if the option is enabled
                    if self.save_default {
                        std::fs::write(&self.file, &default_content).map_err(LoadErrorCause::DefaultWrite)?;
                    }

                    Ok(default_content)
                } else {
                    // no default
                    Err(LoadErrorCause::Read(e))
                }
            }
            Err(e) => Err(LoadErrorCause::Read(e)),
        }
    }
}

impl<'f, F: Fn() -> anyhow::Result<toml::Table> + 'f> DefaultConfigProvider for F {
    fn default_config(&self) -> anyhow::Result<toml::Table> {
        let table = self()?;
        Ok(table)
    }
}

impl<'p, A: Serialize, F: Fn() -> A> AutoDefaultConfigProvider<'p, A, F> {
    /// Creates a new default config provider that use the given `plugins` and general options.
    ///
    /// See the structure documentation for more details.
    pub fn new(plugins: &'p PluginSet, default_general_options: F) -> Self {
        Self {
            plugins,
            default_general_options,
        }
    }
}

impl<'p, A: Serialize, F: Fn() -> A> DefaultConfigProvider for AutoDefaultConfigProvider<'p, A, F> {
    fn default_config(&self) -> anyhow::Result<toml::Table> {
        // generate the default agent config
        let mut config = toml::Table::try_from((self.default_general_options)())?;
        // generate the default plugins configs
        let plugins_table = generate_plugin_configs(self.plugins.metadata(PluginFilter::Enabled))?;
        // make the global config
        config.insert(String::from("plugins"), toml::Value::Table(plugins_table));
        Ok(config)
    }
}

impl DefaultConfigProvider for NoDefaultConfigProvider {
    fn default_config(&self) -> anyhow::Result<toml::Table> {
        Err(anyhow!("no default config available"))
    }
}

/// Replaces the pattern `${VAR_NAME}` by the value of the `VAR_NAME` environment
/// variable.
///
/// The pattern can be escaped to prevent its replacement: `\${NOT_A_VAR}`.
/// If a variable does not exist or is invalid, returns an error.
///
pub fn substitute_env(mut input: &str) -> Result<Cow<str>, InvalidSubstitutionError> {
    // Look for the first substitution.
    let first = input.find("${");
    if first.is_none() {
        // No ${ENV_VAR}: return the string directly
        return Ok(Cow::Borrowed(input));
    }

    // There is at least one substitution to do, we need a new string.
    let mut res = String::with_capacity(input.len());
    let mut next = first;

    // Find each substitution in a loop, and shift the start of `input` to only
    // search in unexplored parts of the input string.
    while let Some(begin) = next {
        let next_start;
        if begin == 0 || input.as_bytes().get(begin - 1) != Some(&b'\\') {
            // ${} not escaped, attempt to perform the variable substitution

            // push chars before the substitution
            res.push_str(&input[..begin]);

            // move forward
            input = &input[begin..];

            // get the env var
            match input.find('}') {
                None => {
                    // unclosed substitution: "${substitution never ends..."
                    return Err(InvalidSubstitutionError::WrongSyntax);
                }
                Some(end) => {
                    // correct substitution syntax: "${VAR_NAME}"
                    let env_var_name = &input[2..end];
                    match std::env::var(env_var_name) {
                        Ok(env_var_value) => {
                            // We have found the environment variable: substitute.
                            res.push_str(&env_var_value);
                        }
                        Err(VarError::NotPresent) => {
                            return Err(InvalidSubstitutionError::Missing(env_var_name.to_owned()))
                        }
                        Err(VarError::NotUnicode(_)) => {
                            return Err(InvalidSubstitutionError::InvalidValue(env_var_name.to_owned()))
                        }
                    }
                    // skip the closing } and continue
                    next_start = end + 1;
                }
            }
        } else {
            // skip the escaped $ and continue
            next_start = begin + 1;

            // push chars before "\$", remove the '\' and keep the '$'
            res.push_str(&input[..(begin - 1)]);
            res.push('$');
        }

        if let Some(more_input) = &input.get(next_start..) {
            // go to the next potential substitution
            input = more_input;
            next = input.find("${");
        } else {
            // end of input, stop
            next = None;
        }
    }
    // add the last part of the input
    res.push_str(input);
    Ok(Cow::Owned(res))
}

/// Merges two toml tables by overriding the content of `original`
/// with the content of `overrides`.
///
/// This function performs a **deep merge**.
pub fn merge_override(original: &mut toml::Table, overrider: toml::Table) {
    for (key, value) in overrider.into_iter() {
        match original.entry(key.clone()) {
            toml::map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(value);
            }
            toml::map::Entry::Occupied(mut occupied_entry) => {
                let existing_value = occupied_entry.get_mut();
                match (existing_value, value) {
                    (toml::Value::Table(map), toml::Value::Table(map_override)) => {
                        merge_override(map, map_override);
                    }
                    (_, value) => {
                        occupied_entry.insert(value);
                    }
                };
            }
        };
    }
}

/// For each plugin in `metadata`, extracts the corresponding config subsection and some
/// standard settings.
///
/// The `config` must contain a `plugins` table with one sub-table for each loaded plugin.
/// The `enabled` value is removed from the sub-tables and used to determine which plugins
/// are enabled.
///
/// # Example
///
/// ```
/// use std::str::FromStr;
/// use alumet::agent::config::extract_plugins_config;
/// use alumet::static_plugins;
///
/// struct A;
/// struct B;
/// #
/// # use alumet::plugin::{AlumetPluginStart, ConfigTable};
/// #
/// impl alumet::plugin::rust::AlumetPlugin for A {
///     fn name() -> &'static str { "a" }
///     // TODO
/// #   fn version() -> &'static str { "0.0.1" }
/// #   fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> { Ok(Box::new(Self)) }
/// #   fn start(&mut self, _: &mut AlumetPluginStart) -> anyhow::Result<()> { Ok(()) }
/// #   fn stop(&mut self) -> anyhow::Result<()> { Ok(()) }
/// #   fn default_config() -> anyhow::Result<Option<ConfigTable>> { Ok(None) }
/// }
/// #
/// impl alumet::plugin::rust::AlumetPlugin for B {
///     fn name() -> &'static str { "b" }
///     // TODO
/// #   fn version() -> &'static str { "0.0.1" }
/// #   fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> { Ok(Box::new(Self)) }
/// #   fn start(&mut self, _: &mut AlumetPluginStart) -> anyhow::Result<()> { Ok(()) }
/// #   fn stop(&mut self) -> anyhow::Result<()> { Ok(()) }
/// #   fn default_config() -> anyhow::Result<Option<ConfigTable>> { Ok(None) }
/// }
/// #
/// # fn prepare_plugin_configs() -> anyhow::Result<()> {
/// let plugins = static_plugins![A, B];
/// let config_example = r#"
///     global_option = "this value is not for plugins"
///
///     [plugins.a]
///     enabled = false  # default is enabled (see plugins.b)
///     key = "value"
///     
///     [plugins.b]
///     option = 123
/// "#;
/// let mut config = toml::Table::from_str(config_example)?;
/// let pc = extract_plugins_config(&mut config)?;
///
/// // let's see what we've got (for the example)
/// let (a_enabled, a_config) = pc.get("a").unwrap();
/// let (b_enabled, b_config) = pc.get("b").unwrap();
///
/// // Plugin "a" is disabled, "b" is enabled.
/// assert!(!a_enabled);
/// assert!(b_enabled);
///
/// // Each plugin has its own config section.
/// assert_eq!(a_config.get("key"), Some(&toml::Value::String(String::from("value"))));
/// assert_eq!(b_config.get("option"), Some(&toml::Value::Integer(123)));
///
/// // The `plugins` table has been removed from the config.
/// assert!(config.get("plugins").is_none());
///
/// // The global options are still there.
/// assert_eq!(config.get("global_option"), Some(&toml::Value::String(String::from("this value is not for plugins"))));
/// # Ok(())
/// # }
/// # // Call the function so that the test runs (the function exists to allow the use of `?`).
/// # prepare_plugin_configs().unwrap();
/// ```
pub fn extract_plugins_config(config: &mut toml::Table) -> Result<BTreeMap<String, (bool, toml::Table)>, BadTypeError> {
    /// Extracts the `enabled` key and remaining configuration entries from a plugin section.
    ///
    /// Returns an error if the section or `enabled` key is of the wrong type.
    fn process_plugin_config(
        plugin_name: &str,
        config_section: toml::Value,
    ) -> Result<(bool, toml::Table), BadTypeError> {
        match config_section {
            toml::Value::Table(mut plugin_config) => {
                // get the TOML value
                let enabled_val = plugin_config
                    .remove("enabled")
                    .or_else(|| plugin_config.remove("enable"))
                    .unwrap_or(toml::Value::Boolean(true));
                // check that the value is of the proper type and turn it into a boolean
                let enabled = enabled_val.as_bool().ok_or_else(|| {
                    BadTypeError::new(format!("plugins.{}.enabled", plugin_name), "boolean", enabled_val)
                })?;
                // done
                Ok((enabled, plugin_config))
            }
            bad => {
                // the value `plugins.{name}` is not a table
                Err(BadTypeError::new(format!("plugins.{}", plugin_name), "table", bad))
            }
        }
    }

    // Remove the `plugins` value from the config and check its type.
    let plugins_table = match config.remove("plugins") {
        Some(toml::Value::Table(t)) => Ok(t),
        Some(bad) => Err(BadTypeError::new(String::from("plugins"), "table", bad)),
        None => Ok(toml::Table::new()),
    }?;

    // Build a map that maps each plugin name to its config
    let mut res = BTreeMap::new();
    for (plugin, section) in plugins_table {
        let (enabled, config) = process_plugin_config(&plugin, section)?;
        res.insert(plugin, (enabled, config));
    }
    Ok(res)
}

/// Generates a table containing the default configuration of each plugin.
pub fn generate_plugin_configs<'p, I: IntoIterator<Item = &'p PluginMetadata>>(
    plugins: I,
) -> Result<toml::Table, PluginDefaultConfigError> {
    let plugins = plugins.into_iter();
    let (lower, _) = plugins.size_hint();
    let mut table = toml::Table::with_capacity(lower);
    for p in plugins {
        let plugin_config = (p.default_config)().map_err(|e| PluginDefaultConfigError {
            plugin_name: p.name.clone(),
            source: e,
        })?;

        if let Some(config) = plugin_config {
            table.insert(p.name.clone(), toml::Value::Table(config.0));
        }
    }
    Ok(table)
}

pub mod error {
    use std::{io, path::PathBuf};
    use thiserror::Error;

    /// [`Loader::load`](super::Loader::load) failed.
    #[derive(Error, Debug)]
    #[error("could not load config from '{config_file}'")]
    pub struct LoadError {
        /// The configuration file that was tentatively loaded.
        pub config_file: PathBuf,

        /// What caused the error.
        #[source]
        pub(super) kind: LoadErrorCause,
    }

    /// What made the configuration loading fail?
    #[derive(Error, Debug)]
    pub(super) enum LoadErrorCause {
        /// I/O error: reading the configuration file failed.
        #[error("read failed")]
        Read(#[source] io::Error),

        /// The loader tried to generate a default configuration (because the config file did not exist),
        /// but the generation failed.
        #[error("default provider returned an error")]
        DefaultProvider(#[source] anyhow::Error),

        /// A default configuration was generated but could not be saved to the file.
        #[error("write (of default config) failed")]
        DefaultWrite(#[source] io::Error),

        /// The config file was read but environment variable substitution failed.
        #[error("env var substitution failed")]
        Substitution(#[from] InvalidSubstitutionError),

        /// The config file was read but could not be parsed to a valid TOML structure
        /// (after environment variable substitution).
        #[error("invalid TOML config")]
        InvalidToml(#[from] toml::de::Error),
    }

    /// Environment variable substitution failed.
    #[derive(Error, Debug, PartialEq)]
    pub enum InvalidSubstitutionError {
        /// The environment variable does not exist.
        #[error("the environment variable {0} does not exist")]
        Missing(String),
        /// The value of the variable is not valid UTF-8.
        #[error("value of env var {0} is not valid UTF-8")]
        InvalidValue(String),
        /// The name of the variable contains a forbidden character.
        #[error("env var name {0} is not valid")]
        InvalidName(String),
        /// The substitution syntax has not been used properly.
        #[error("wrong use of the substitution syntax, it should be ${{ENV_VAR}}")]
        WrongSyntax,
    }

    /// A value of the TOML configuration had an unexpected type.
    #[derive(Error, Debug)]
    #[error("unexpected type for {path}: expected {expected}, got {actual}")]
    pub struct BadTypeError {
        pub path: String,
        pub expected: &'static str,
        pub actual: &'static str,
    }

    impl BadTypeError {
        pub fn new(path: String, expected: &'static str, actual: toml::Value) -> Self {
            Self {
                path,
                expected,
                actual: actual.type_str(),
            }
        }
    }

    /// A plugin failed to generate a default configuration.
    #[derive(Error, Debug)]
    #[error("plugin {plugin_name} failed to generate a default configuration")]
    pub struct PluginDefaultConfigError {
        pub plugin_name: String,

        #[source]
        pub(super) source: anyhow::Error,
    }
}

#[cfg(test)]
mod tests_substitute_env {
    use std::borrow::Cow;

    use super::{substitute_env, InvalidSubstitutionError};

    // This environment variable exist both at compile time and runtime.
    const ENV_VAR_NAME: &str = "CARGO_PKG_NAME";
    const ENV_VAR_VALUE: &str = env!("CARGO_PKG_NAME");
    const SUBSTITUTION: &str = "${CARGO_PKG_NAME}";
    const ESCAPED_SUBST: &str = "\\${CARGO_PKG_NAME}";

    #[test]
    fn no_substitution() {
        let input = "";
        assert_eq!(Cow::Borrowed(input), substitute_env(input).unwrap());

        let input = "
            config_option = 123
            
            [table]
            list = [a, b, 'd', 1.5]
        ";
        assert_eq!(Cow::Borrowed(input), substitute_env(input).unwrap());

        let input = "
            config_option = 123
            
            [table]
            list = [a, b, '$', 1.5]
        ";
        assert_eq!(Cow::Borrowed(input), substitute_env(input).unwrap());
    }

    #[test]
    fn basic() {
        assert_eq!(
            std::env::var(ENV_VAR_NAME).as_deref(),
            Ok(ENV_VAR_VALUE),
            "env var {} should be the same at compile-time and runtime",
            ENV_VAR_NAME
        );

        let input = SUBSTITUTION;
        let expected = ENV_VAR_VALUE;
        assert_eq!(
            expected,
            substitute_env(input).unwrap(),
            "wrong result on input: {}",
            input
        );

        let input = format!("something${SUBSTITUTION}");
        let expected = format!("something${ENV_VAR_VALUE}");
        assert_eq!(expected, substitute_env(&input).unwrap());

        let input = format!("${SUBSTITUTION}something");
        let expected = format!("${ENV_VAR_VALUE}something");
        assert_eq!(expected, substitute_env(&input).unwrap());

        let input = format!("list = [a, b, '${SUBSTITUTION}', 1.5]");
        let expected = input.replace(SUBSTITUTION, ENV_VAR_VALUE);
        assert_eq!(expected, substitute_env(&input).unwrap());
    }

    #[test]
    fn multiple() {
        assert_eq!(
            std::env::var(ENV_VAR_NAME).as_deref(),
            Ok(ENV_VAR_VALUE),
            "env var {} should be the same at compile-time and runtime",
            ENV_VAR_NAME
        );

        let input = format!(
            r#"
            config_option = "${SUBSTITUTION}"
            
            [table]
            list = [a, b, '${SUBSTITUTION}', 1.5]
            echo = "${SUBSTITUTION}${SUBSTITUTION}"
            
            [[${SUBSTITUTION}.${SUBSTITUTION}]]
        "#
        );
        let expected = input.replace(SUBSTITUTION, ENV_VAR_VALUE);
        assert_eq!(expected, substitute_env(&input).unwrap());
    }

    #[test]
    fn escaped() {
        assert_eq!(
            std::env::var(ENV_VAR_NAME).as_deref(),
            Ok(ENV_VAR_VALUE),
            "env var {} should be the same at compile-time and runtime",
            ENV_VAR_NAME
        );

        let input = ESCAPED_SUBST;
        let expected = SUBSTITUTION;
        assert_eq!(
            expected,
            substitute_env(input).unwrap(),
            "wrong result on input: {}",
            input
        );

        let input = format!("something${ESCAPED_SUBST}");
        let expected = format!("something${SUBSTITUTION}");
        assert_eq!(expected, substitute_env(&input).unwrap());

        let input = format!("${ESCAPED_SUBST}something");
        let expected = format!("${SUBSTITUTION}something");
        assert_eq!(expected, substitute_env(&input).unwrap());

        let input = format!("${ESCAPED_SUBST}${ESCAPED_SUBST}");
        let expected = format!("${SUBSTITUTION}${SUBSTITUTION}");
        assert_eq!(expected, substitute_env(&input).unwrap());
    }

    #[test]
    fn escaped_unescaped_mix() {
        let input = format!(" ${ESCAPED_SUBST}  ${SUBSTITUTION}");
        let expected = format!(" ${SUBSTITUTION}  ${ENV_VAR_VALUE}");
        assert_eq!(expected, substitute_env(&input).unwrap());
    }

    #[test]
    fn unclosed() {
        let input = "${";
        assert_eq!(substitute_env(input), Err(InvalidSubstitutionError::WrongSyntax));

        let input = "abc${";
        assert_eq!(substitute_env(input), Err(InvalidSubstitutionError::WrongSyntax));

        let input = "${UNCLOSED_VAR ${";
        assert_eq!(substitute_env(input), Err(InvalidSubstitutionError::WrongSyntax));

        let input = "k = true\n${UNCLOSED_VAR\ntest = 1";
        assert_eq!(substitute_env(input), Err(InvalidSubstitutionError::WrongSyntax));
    }
}
