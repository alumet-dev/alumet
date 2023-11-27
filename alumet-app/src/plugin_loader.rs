use std::{
    error::Error,
    ffi::{c_char, CStr},
    path::{Path, PathBuf}, collections::HashMap,
};

use alumet_api::{
    config::{self, ConfigTable},
    plugin::{ffi, Plugin, PluginError, PluginInfo, PluginResult},
};
use libc::c_void;
use libloading::{Library, Symbol};

/// A plugin initialized from a dynamic library (aka. shared library).
struct DylibPlugin {
    name: String,
    version: String,
    start_fn: ffi::StartFn,
    stop_fn: ffi::StopFn,
    drop_fn: ffi::DropFn,
    // the library must stay loaded for the symbols to be valid
    library: Library,
    instance: *mut c_void,
}

impl Plugin for DylibPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn start(
        &mut self,
        metrics: &mut alumet_api::metric::MetricRegistry,
        sources: &mut alumet_api::plugin::SourceRegistry,
        outputs: &mut alumet_api::plugin::OutputRegistry,
    ) -> Result<(), alumet_api::plugin::PluginError> {
        (self.start_fn)(self.instance); // TODO, metrics, sources, outputs);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), alumet_api::plugin::PluginError> {
        (self.stop_fn)(self.instance); // TODO error handling for ffi
        Ok(())
    }
}

impl Drop for DylibPlugin {
    fn drop(&mut self) {
        // When the external plugin is dropped, call the external code that allocated the
        // `instance` struct, in order to de-allocate it. The external code should also free
        // the resources it has previously opened, if any.
        (self.drop_fn)(self.instance);
    }
}

pub fn load_cdylib(file: &Path) -> Result<PluginInfo, Box<dyn Error>> {
    log::debug!("loading dynamic library {}", file.display());
    // load the library and the symbols we need to initialize the plugin
    // BEWARE: to load a constant of type `T` from the shared library, a `Symbol<*const T>` or `Symbol<*mut T>` must be used.
    // However, to load a function of type `fn(A,B) -> R`, a `Symbol<extern fn(A,B) -> R>` must be used.
    let lib = unsafe { Library::new(file)? };
    log::debug!("library loaded");
    let sym_name: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_NAME\0")? };
    let sym_version: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_VERSION\0")? };
    let sym_init: Symbol<ffi::InitFn> = unsafe { lib.get(b"plugin_init\0")? };
    let sym_start: Symbol<ffi::StartFn> = unsafe { lib.get(b"plugin_start\0")? };
    let sym_stop: Symbol<ffi::StopFn> = unsafe { lib.get(b"plugin_stop\0")? };
    let sym_drop: Symbol<ffi::DropFn> = unsafe { lib.get(b"plugin_drop\0")? };

    // todo add LOCOMEN_VERSION and check that the plugin is compatible
    log::debug!("symbols loaded");

    // convert the strings to Rust strings
    let name = unsafe { CStr::from_ptr(**sym_name) }.to_str()?.to_owned();
    let version = unsafe { CStr::from_ptr(**sym_version) }.to_str()?.to_owned();
    log::debug!("plugin found: {name} v{version}");

    // extract the function pointers from the Symbol, to get around lifetime constraints
    let init_fn = *sym_init;
    let start_fn = *sym_start;
    let stop_fn = *sym_stop;
    let drop_fn = *sym_drop;

    // wrap the plugin info in a Rust struct, to allow the plugin to be initialized later
    let initializable_info = PluginInfo {
        name: name.clone(),
        version: version.clone(),
        init: Box::new(move |config| {
            // initialize the plugin
            let external_plugin = init_fn(config);
            log::debug!("init called from Rust");

            if external_plugin.is_null() {
                return Err(PluginError::External {
                    description: "plugin_init returned null".to_owned(),
                });
            }

            // wrap the external plugin in a nice Rust struct
            let plugin = DylibPlugin {
                name,
                version,
                start_fn,
                stop_fn,
                drop_fn,
                library: lib,
                instance: external_plugin,
            };
            Ok(Box::new(plugin))
        }),
    };

    Ok(initializable_info)
}

/// Initializes a plugin, using its [`PluginInfo`] and config table (not the global configuration).
pub fn initialize(plugin: PluginInfo, config: toml::Table) -> PluginResult<Box<dyn Plugin>> {
    let mut ffi_config = ConfigTable::new(config).map_err(|err| PluginError::Config {
        description: "conversion to ffi-safe configuration failed".into(),
        source: Some(err.into()),
    })?;
    let plugin_instance = (plugin.init)(&mut ffi_config)?;
    Ok(plugin_instance)
}

pub fn plugin_subconfig(plugin: &PluginInfo, global_config: &mut toml::Table) -> PluginResult<toml::Table> {
    let name = &plugin.name;
    let sub_config = global_config.remove(name);
    match sub_config {
        Some(toml::Value::Table(t)) => Ok(t),
        Some(bad_value) => Err(PluginError::Config {
            description: format!(
                "invalid plugin configuration for '{name}': the value must be a table, not a {}.",
                bad_value.type_str()
            ),
            source: None,
        }),
        None => Err(PluginError::Config {
            description: format!("missing plugin configuration for '{name}'"),
            source: None,
        }),
    }
}

pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.insert(plugin.name().into(), plugin);
    }
    
    pub fn get_mut(&self, name: &str) -> Option<&mut dyn Plugin> {
        self.plugins.get_mut(name).map(|boxed| boxed.as_mut())
    }
}
