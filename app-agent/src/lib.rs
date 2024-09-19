use std::path::PathBuf;

use env_logger::Env;

pub mod agent_util;
pub mod exec_process;
pub mod options;

/// Returns the path to the Alumet agent that is being executed.
pub fn resolve_application_path() -> std::io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
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
