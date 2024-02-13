use std::{
    collections::HashMap,
    error::Error,
    ffi::{c_char, CStr},
    path::Path,
};

// use alumet_api::{
//     AlumetStart,
//     config::{self, ConfigTable},
//     plugin::{ffi, Plugin, PluginError, PluginInfo},
// };
use libc::c_void;
use libloading::{Library, Symbol};

use crate::{config::ConfigTable, plugin::{version::{self, Version}, PluginErrorKind::InitFailure}};

use super::{
    dyn_ffi, AlumetStart, Plugin, PluginError,
    PluginErrorKind::{self, InvalidConfiguration},
    PluginInfo,
};

/// A plugin initialized from a dynamic library (aka. shared library).
struct DylibPlugin {
    name: String,
    version: String,
    start_fn: dyn_ffi::StartFn,
    stop_fn: dyn_ffi::StopFn,
    drop_fn: dyn_ffi::DropFn,
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

    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), PluginError> {
        (self.start_fn)(self.instance, alumet);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PluginError> {
        (self.stop_fn)(self.instance); // TODO error handling for ffi
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
        (self.drop_fn)(self.instance);
    }
}

/// Loads a dynamic plugin from a shared library file, and returns a [`PluginInfo`] that allows to initialize the plugin.
pub fn load_cdylib(file: &Path) -> Result<PluginInfo, Box<dyn Error>> {
    log::debug!("loading dynamic library {}", file.display());
    // load the library and the symbols we need to initialize the plugin
    // BEWARE: to load a constant of type `T` from the shared library, a `Symbol<*const T>` or `Symbol<*mut T>` must be used.
    // However, to load a function of type `fn(A,B) -> R`, a `Symbol<extern fn(A,B) -> R>` must be used.
    let lib = unsafe { Library::new(file)? };
    log::debug!("library loaded");

    let sym_name: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_NAME\0")? };
    let sym_plugin_version: Symbol<*const *const c_char> = unsafe { lib.get(b"PLUGIN_VERSION\0")? };
    let sym_alumet_version: Symbol<*const *const c_char> = unsafe { lib.get(b"ALUMET_VERSION\0")? };
    let sym_init: Symbol<dyn_ffi::InitFn> = unsafe { lib.get(b"plugin_init\0")? };
    let sym_start: Symbol<dyn_ffi::StartFn> = unsafe { lib.get(b"plugin_start\0")? };
    let sym_stop: Symbol<dyn_ffi::StopFn> = unsafe { lib.get(b"plugin_stop\0")? };
    let sym_drop: Symbol<dyn_ffi::DropFn> = unsafe { lib.get(b"plugin_drop\0")? };

    log::debug!("symbols loaded");

    // convert the strings to Rust strings
    let name = unsafe { CStr::from_ptr(**sym_name) }.to_str()?.to_owned();
    let version = unsafe { CStr::from_ptr(**sym_plugin_version) }.to_str()?.to_owned();
    let required_alumet_version = unsafe { CStr::from_ptr(**sym_alumet_version) }.to_str()?.to_owned();
    log::debug!("plugin found: {name} v{version}  (requires ALUMET v{required_alumet_version})");
    
    // check the required ALUMET version
    let required_alumet_version = Version::parse(&required_alumet_version).unwrap(); // todo report error
    if !Version::alumet().can_load(required_alumet_version) {
        todo!("invalid ALUMET version requirement");
    }

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
                return Err(PluginError::with_description(InitFailure, "plugin_init returned null"));
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
pub fn initialize(plugin: PluginInfo, config: toml::Table) -> Result<Box<dyn Plugin>, PluginError> {
    let mut ffi_config = ConfigTable::new(config).map_err(|err| {
        PluginError::with_cause(InvalidConfiguration, "conversion to ffi-safe configuration failed", err)
    })?;
    let plugin_instance = (plugin.init)(&mut ffi_config)?;
    Ok(plugin_instance)
}

pub fn plugin_subconfig(plugin: &PluginInfo, global_config: &mut toml::Table) -> Result<toml::Table, PluginError> {
    let name = &plugin.name;
    let sub_config = global_config.remove(name);
    match sub_config {
        Some(toml::Value::Table(t)) => Ok(t),
        Some(bad_value) => Err(PluginError::with_description(
            InvalidConfiguration,
            &format!(
                "invalid plugin configuration for '{name}': the value must be a table, not a {}.",
                bad_value.type_str()
            ),
        )),
        None => Err(PluginError::with_description(
            InvalidConfiguration,
            &format!("missing plugin configuration for '{name}'"),
        )),
    }
}

pub struct PluginRegistry {
    plugins: HashMap<String, Box<dyn Plugin>>,
}

impl PluginRegistry {
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        self.plugins.insert(plugin.name().into(), plugin);
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut dyn Plugin> {
        self.plugins.get_mut(name).map(|b| &mut **b as _)
        // the cast is necessary here to coerce the lifetime
        // `&mut dyn Plugin + 'static` to `&mut dyn Plugin + 'a`
    }
}
