//! Static and dynamic plugins.
//!
//! Plugins are an essential part of Alumet, as they provide the
//! [`Source`]s, [`Transform`]s and [`Output`]s.
//!
//! ## Plugin lifecycle
//! Every plugin follow these steps:
//!
//! 1. **Metadata loading**: the Alumet application loads basic information about the plugin.
//! For static plugins, this is done at compile time (cargo takes care of the dependencies).
//!
//! 2. **Initialization**: the plugin is initialized, a value that implements the [`Plugin`] trait is created.
//! During the initialization phase, the plugin can read its configuration.
//!
//! 3. **Start-up**: the plugin is started via its [`start`](Plugin::start) method.
//! During the start-up phase, the plugin can create metrics, register new pipeline elements
//! (sources, transforms, outputs), and more.
//!
//! 4. **Operation**: the measurement pipeline has started, and the elements registered by the plugin
//! are in use. Alumet takes care of the lifetimes and of the triggering of those elements.
//!
//! 5. **Stop**: the elements registered by the plugin are stopped and dropped.
//! Then, [`stop`](Plugin::stop) is called.
//!
//! 6. **Drop**: like any Rust value, the plugin is dropped when it goes out of scope.
//! To customize the destructor of your static plugin, implement the [`Drop`] trait on your plugin structure.
//! For a dynamic plugin, modify the `plugin_drop` function.
//!
//! ## Static plugins
//!
//! A static plugin is a plugin that is included in the Alumet measurement application
//! at compile-time. The measurement tool and the static plugin are compiled together,
//! in a single executable binary.
//!
//! To create a static plugin in Rust, make a new library crate that depends on:
//! - the core of Alumet (the `alumet` crate).
//! - the `anyhow` crate
//!
//! To do this, here is an example (bash commands):
//! ```sh
//! cargo init --lib my-plugin
//! cd my-plugin
//! cargo add alumet anyhow
//! ```
//!
//! In your library, define a structure for your plugin, and implement the [`rust::AlumetPlugin`] trait for it.
//! ```no_run
//! use alumet::plugin::{rust::AlumetPlugin, AlumetPluginStart, ConfigTable};
//!
//! struct MyPlugin {}
//!
//! impl AlumetPlugin for MyPlugin {
//!     fn name() -> &'static str {
//!         "my-plugin"
//!     }
//!
//!     fn version() -> &'static str {
//!         "0.1.0"
//!     }
//!
//!     fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
//!         // You can read the config and store some settings in your structure.
//!         Ok(Box::new(MyPlugin {}))
//!     }
//!
//!     fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
//!         println!("My first plugin is starting!");
//!         Ok(())
//!     }
//!
//!     fn stop(&mut self) -> anyhow::Result<()> {
//!         println!("My first plugin is stopping!");
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Finally, modify the measurement application in the following ways:
//! 1. Add a dependency to your plugin crate (for example with `cargo add my-plugin --path=path/to/my-plugin`).
//! 2. Modify your `main` to initialize and load the plugin. See [`Agent`](crate::agent::Agent).
//!
//! ## Dynamic plugins
//!
//! WIP
//!
use self::rust::AlumetPlugin;

#[cfg(feature = "dynamic")]
pub mod dynload;

pub mod event;
mod phases;
pub mod rust;
pub mod util;
pub(crate) mod version;

pub use phases::{AlumetPluginStart, AlumetPostStart, AlumetPreStart};

/// Plugin metadata, and a function that allows to initialize the plugin.
pub struct PluginMetadata {
    /// Name of the plugin, must be unique.
    pub name: String,
    /// Version of the plugin, should follow semantic versioning (of the form `x.y.z`).
    pub version: String,
    /// Function that initializes the plugin.
    pub init: Box<dyn FnOnce(ConfigTable) -> anyhow::Result<Box<dyn Plugin>>>,
    /// Function that returns a default configuration for the plugin, or None
    /// if the plugin has no configurable option.
    ///
    /// The default config is used to generate the configuration file of the
    /// Alumet agent, in case it does not exist. In other cases, the default
    /// config returned by this function is not used, including when
    pub default_config: Box<dyn Fn() -> anyhow::Result<Option<ConfigTable>>>,
}

impl PluginMetadata {
    /// Build a metadata structure for a static plugin that implements [`AlumetPlugin`].
    pub fn from_static<P: AlumetPlugin + 'static>() -> Self {
        Self {
            name: P::name().to_owned(),
            version: P::version().to_owned(),
            init: Box::new(|conf| P::init(conf).map(|p| p as _)),
            default_config: Box::new(P::default_config),
        }
    }
}

/// A configuration table for plugins.
///
/// `ConfigTable` is currently a wrapper around [`toml::Table`].
/// However, you probably don't need to add a dependency on the `toml` crate,
/// since Alumet provides functions to easily serialize and deserialize configurations
/// with `serde`.
///
/// ## Example
///
/// ```
/// use serde::{Serialize, Deserialize};
/// use alumet::plugin::ConfigTable;
/// use alumet::plugin::rust::{serialize_config, deserialize_config};
///
/// #[derive(Serialize, Deserialize)]
/// struct MyConfig {
///     field: String
/// }
///
/// // serialize struct to config
/// let my_struct = MyConfig { field: String::from("value") };
/// let serialized: ConfigTable = serialize_config(my_struct).expect("serialization failed");
///
/// // deserialize config to struct
/// let my_table: ConfigTable = serialized;
/// let deserialized: MyConfig = deserialize_config(my_table).expect("deserialization failed");
/// ```
#[derive(Debug, Clone)]
pub struct ConfigTable(pub toml::Table);

/// Trait for plugins.
///
/// ## Note for plugin authors
///
/// You should _not_ implement this trait manually.
///
/// If you are writing a plugin in Rust, implement [`AlumetPlugin`] instead.
/// If you are writing a plugin in C, you need to define the right symbols in your shared library.
pub trait Plugin {
    /// The name of the plugin. It must be unique: two plugins cannot have the same name.
    fn name(&self) -> &str;

    /// The version of the plugin, for instance `"1.2.3"`. It should adhere to semantic versioning.
    fn version(&self) -> &str;

    /// Starts the plugin, allowing it to register metrics, sources and outputs.
    ///
    /// ## Plugin restart
    /// A plugin can be started and stopped multiple times, for instance when ALUMET switches from monitoring to profiling mode.
    /// [`Plugin::stop`] is guaranteed to be called between two calls of [`Plugin::start`].
    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()>;

    /// Stops the plugin.
    ///
    /// This method is called _after_ all the metrics, sources and outputs previously registered
    /// by [`Plugin::start`] have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()>;
    
    /// Function called after the startup phase but before the operation phase,
    /// i.e. the measurement pipeline has not started yet.
    ///
    /// It can be used, for instance, to obtain the list of all registered metrics.
    fn pre_pipeline_start(&mut self, alumet: &mut AlumetPreStart) -> anyhow::Result<()>;

    /// Function called after the beginning of the operation phase,
    /// i.e. the measurement pipeline has started.
    ///
    /// It can be used, for instance, to obtain a [`ControlHandle`](crate::pipeline::runtime::ControlHandle)
    /// of the pipeline.
    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()>;
}
