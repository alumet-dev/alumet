use std::path::PathBuf;

use env_logger::Env;

pub mod agent_util;
pub mod config_ops;
pub mod exec_process;
pub mod options;
pub mod word_distance;

/// Returns the path to the Alumet agent that is being executed.
pub fn resolve_application_path() -> std::io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

pub fn relative_app_path_string() -> PathBuf {
    resolve_application_path()
        .map(|exe| {
            std::env::current_dir()
                .ok()
                .and_then(|wdir| exe.strip_prefix(wdir).ok())
                .map(|p| p.to_path_buf())
                .unwrap_or(exe)
        })
        .unwrap_or_else(|_| "path/to/alumet-agent".into())
}

/// Initializes the global logger.
///
/// Call this first!
pub fn init_logger() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Print a warning if we are running in debug mode.
    #[cfg(debug_assertions)]
    {
        log::warn!("DEBUG assertions are enabled, this build of Alumet is fine for debugging, but not for production.");
    }
}
