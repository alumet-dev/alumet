//! Loading of dynamic plugins from shared libraries.

use std::{
    collections::HashMap,
    ffi::{c_char, CStr},
    path::Path,
};

use libc::c_void;
use libloading::{Library, Symbol};

use alumet::plugin::{
    version::{self, Version},
    AlumetPreStart,
};
use alumet::plugin::{AlumetPluginStart, AlumetPostStart, ConfigTable, Plugin, PluginMetadata};

/// A plugin initialized from a dynamic library (aka. shared library).
struct DylibPlugin {
    name: String,
    version: String,
    start_fn: super::PluginStartFn,
    stop_fn: super::PluginStopFn,
    drop_fn: super::DropFn,
    // the library must stay loaded for the symbols to be valid
    _library: Library,
    instance: *mut c_void,
}

impl Plugin for DylibPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        (self.start_fn)(self.instance, alumet); // TODO error handling for ffi
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        (self.stop_fn)(self.instance); // TODO error handling for ffi
        Ok(())
    }

    fn pre_pipeline_start(&mut self, _alumet: &mut AlumetPreStart) -> anyhow::Result<()> {
        // TODO
        Ok(())
    }

    fn post_pipeline_start(&mut self, _alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        // TODO
        Ok(())
    }
}

impl Drop for DylibPlugin {
    fn drop(&mut self) {
        // When the external plugin is dropped, call the external code that allocated the
        // `instance` struct, in order to de-allocate it. The external code should also free
        // the resources it has previously opened, if any.
        //
        // **Rule of thumb**: Rust allocations are deallocated by Rust code,
        // C allocations (malloc) are deallocated by C code (free).
        unsafe { (self.drop_fn)(self.instance) };
    }
}

/// Error during the loading of a dynamic plugin.
#[derive(Debug)]
pub enum LoadError {
    /// Unable to load something the shared library.
    LibraryLoad(libloading::Error),
    /// A symbol loaded from the library contains an invalid value.
    InvalidSymbol(&'static str, Box<dyn std::error::Error + Send + Sync>),
    /// The plugin is not compatible with this version of ALUMET core.
    IncompatiblePlugin {
        plugin_alumet_version: Version,
        current_alumet_version: Version,
    },
    /// `plugin_init` failed.
    PluginInit,
}

/// Registry of plugins, to initialize dynamic plugins one by one.
pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
}

/// Loads a dynamic plugin from a shared library file, and returns a [`PluginMetadata`] that allows to initialize the plugin.
///
/// # Required symbols
/// To be valid, a dynamic plugin must declare the following shared symbols:
/// - `PLUGIN_NAME: *const c_char`: the name of the plugin, as a null-terminated string
/// - `PLUGIN_VERSION: *const c_char`: the version of the plugin, of the form "x.y.z" where x,y,z are integers
/// - `ALUMET_VERSION: *const c_char`: the version of alumet that this plugin requires, of the form "x.y.z"
/// - `plugin_init: PluginInitFn`: see [`super::PluginInitFn`]
/// - `plugin_start: PluginStartFn`: see [`super::PluginStartFn`]
/// - `plugin_stop: PluginStopFn`: see [`super::PluginStopFn`]
/// - `plugin_drop: DropFn`: see [`super::DropFn`]
///
/// # Declaration in Rust
/// Declaring such variables and symbols in the Rust language would look like the following:
/// ```ignore
/// #[no_mangle]
/// pub static PLUGIN_NAME: &[u8] = b"my-plugin\0";
/// #[no_mangle]
/// pub static PLUGIN_VERSION: &[u8] = b"0.0.1\0";
/// #[no_mangle]
/// pub static ALUMET_VERSION: &[u8] = b"0.1.0\0";
///
/// #[no_mangle]
/// pub extern "C" fn plugin_init(config: &ConfigTable) -> *mut MyPluginStruct {}
/// #[no_mangle]
/// pub extern "C" fn plugin_start(plugin: &mut MyPluginStruct, alumet: &mut AlumetPluginStart) {}
/// #[no_mangle]
/// pub extern "C" fn plugin_stop(plugin: &mut MyPluginStruct) {}
/// #[no_mangle]
/// pub extern "C" fn plugin_drop(plugin: *mut MyPluginStruct) {}
/// ```
///
/// # Declaration in C
/// Declaring such variables and symbols in the C language would look like the following:
/// ```ignore
/// PLUGIN_API const char *PLUGIN_NAME = "my-plugin";
/// PLUGIN_API const char *PLUGIN_VERSION = "0.0.1";
/// PLUGIN_API const char *ALUMET_VERSION = "0.1.0";
///
/// PLUGIN_API MyPluginStruct *plugin_init(const ConfigTable *config) {}
/// PLUGIN_API void plugin_start(MyPluginStruct *plugin, AlumetPluginStart *alumet) {}
/// PLUGIN_API void plugin_stop(MyPluginStruct *plugin) {}
/// PLUGIN_API void plugin_drop(MyPluginStruct *plugin) {}
/// ```
///
/// # Exporting the symbols properly
/// You must ensure that the aforementioned symbols are properly exported, so that the ALUMET agent can
/// load them. But be careful not to export private variables and functions, as they can cause conflicts
/// between different plugins.
///
/// In Rust, the recommended way to do that is to make your crate a `cdylib` crate by putting the following
/// in your `Cargo.toml` file:
/// ```toml
/// [lib]
/// crate-type = ["cdylib"]
/// ```
/// and to prefix the symbols to export with `#[no_mangle]`, as shown above.
///
/// In C, the recommended way to do that is to compile with the following flags:
/// ```ignore
/// -shared -fPIC -fvisibility=hidden
/// ```
/// and to prefix the symbols to export with the `PLUGIN_API` macro provided by the ALUMET header file,
/// as shown above.
pub fn load_cdylib(file: &Path) -> Result<PluginMetadata, LoadError> {
    log::debug!("loading dynamic library {}", file.display());

    // load the library and the symbols we need to initialize the plugin
    // BEWARE: to load a constant of type `T` from the shared library, a `Symbol<*const T>` or `Symbol<*mut T>` must be used.
    // However, to load a function of type `fn(A,B) -> R`, a `Symbol<extern fn(A,B) -> R>` must be used.
    let lib = unsafe { Library::new(file)? };
    log::debug!("library loaded");

    let sym_name: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_NAME\0")? };
    let sym_plugin_version: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_VERSION\0")? };
    let sym_alumet_version: Symbol<*const *const c_char> = unsafe { lib.get(b"ALUMET_VERSION\0")? };
    let sym_init: Symbol<super::PluginInitFn> = unsafe { lib.get(b"plugin_init\0")? };
    let sym_start: Symbol<super::PluginStartFn> = unsafe { lib.get(b"plugin_start\0")? };
    let sym_stop: Symbol<super::PluginStopFn> = unsafe { lib.get(b"plugin_stop\0")? };
    let sym_drop: Symbol<super::DropFn> = unsafe { lib.get(b"plugin_drop\0")? };

    // if this symbol is none, there is no default config
    // (this means that it is optional to define `plugin_default_config`)
    let sym_default_config: Option<Symbol<super::PluginDefaultConfigFn>> =
        unsafe { lib.get(b"plugin_default_config\0") }.ok();

    log::debug!("symbols loaded");

    // convert the C strings to Rust strings, and wraps errors in LoadError::InvalidSymbol
    fn sym_to_string(sym: &Symbol<*const *const c_char>, name: &'static str) -> Result<String, LoadError> {
        unsafe { CStr::from_ptr(***sym) }
            .to_str()
            .map_err(|e| LoadError::InvalidSymbol(name, e.into()))
            .map(|v| v.to_owned())
    }

    let name = sym_to_string(&sym_name, "PLUGIN_NAME")?;
    let version = sym_to_string(&sym_plugin_version, "PLUGIN_VERSION")?;
    let alumet_version = sym_to_string(&sym_alumet_version, "ALUMET_VERSION")?;
    log::debug!("plugin found: {name} v{version}  (requires ALUMET v{alumet_version})");

    // get the ALUMET version required by the plugin
    let plugin_alumet_version =
        Version::parse(&alumet_version).map_err(|e| LoadError::InvalidSymbol("ALUMET_VERSION", e.into()))?;

    // check that it matches the current ALUMET version
    let current_alumet_version = Version::alumet();
    if !current_alumet_version.can_load(&plugin_alumet_version) {
        return Err(LoadError::IncompatiblePlugin {
            plugin_alumet_version,
            current_alumet_version,
        });
    }

    // extract the function pointers from the Symbol, to get around lifetime constraints
    let init_fn = *sym_init;
    let start_fn = *sym_start;
    let stop_fn = *sym_stop;
    let drop_fn = *sym_drop;
    let default_config_fn = sym_default_config.map(|sym| *sym);

    // wrap the plugin info in a Rust struct, to allow the plugin to be initialized later
    let initializable_info = PluginMetadata {
        name: name.clone(),
        version: version.clone(),
        init: Box::new(move |config| {
            // initialize the plugin
            let external_plugin = init_fn(&config.0);
            log::debug!("init called from Rust");

            if external_plugin.is_null() {
                return Err(LoadError::PluginInit.into());
            }

            // wrap the external plugin in a nice Rust struct
            let plugin = DylibPlugin {
                name,
                version,
                start_fn,
                stop_fn,
                drop_fn,
                _library: lib,
                instance: external_plugin,
            };
            Ok(Box::new(plugin))
        }),
        default_config: match default_config_fn {
            Some(f) => Box::new(move || {
                let mut config_to_fill = toml::Table::new();
                log::debug!("filling default config");
                f(&mut config_to_fill);
                log::debug!("default config filled");
                Ok(Some(ConfigTable(config_to_fill)))
            }),
            None => Box::new(|| Ok(None)),
        },
    };

    Ok(initializable_info)
}

impl std::error::Error for LoadError {}
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::LibraryLoad(err) => write!(f, "failed to load shared library: {err}"),
            LoadError::InvalidSymbol(name, err) => write!(f, "invalid value for symbol {name}: {err}"),
            LoadError::PluginInit => write!(f, "plugin_init returned NULL"),
            LoadError::IncompatiblePlugin { plugin_alumet_version, current_alumet_version } => write!(f, "plugin requires ALUMET v{plugin_alumet_version}, which is incompatible with current ALUMET v{current_alumet_version}"),
        }
    }
}
impl From<libloading::Error> for LoadError {
    fn from(value: libloading::Error) -> Self {
        LoadError::LibraryLoad(value)
    }
}
impl From<version::Error> for LoadError {
    fn from(value: version::Error) -> Self {
        LoadError::InvalidSymbol("ALUMET_VERSION", Box::new(value))
    }
}

impl PluginRegistry {
    /// Adds a plugin to the registry.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.insert(plugin.name().into(), plugin);
    }

    /// Finds a plugin by its name.
    pub fn get_mut(&mut self, name: &str) -> Option<&mut dyn Plugin> {
        self.plugins.get_mut(name).map(|b| &mut **b as _)
        // the cast is necessary here to coerce the lifetime
        // `&mut dyn Plugin + 'static` to `&mut dyn Plugin + 'a`
    }
}
