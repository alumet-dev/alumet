use std::path::PathBuf;

use env_logger::Env;

pub mod exec_hints;
pub mod word_distance;

/// Returns the absolute path of the currently running executable.
pub fn absolute_exe_path() -> std::io::Result<PathBuf> {
    std::env::current_exe()?.canonicalize()
}

/// Returns the path of the currently running executable, relative
/// to the current working directory.
///
/// # Errors
///
/// If the path of the executable cannot be obtained, an error is returned.
/// See [`std::env::current_exe`].
///
/// If the current directory does not exist or if the current user does not
/// have the necessary permissions, the absolute path is returned instead
/// of the relative one. Use [`std::path::Path::is_relative`] to check whether
/// the returned path is relative or absolute.
pub fn relative_exe_path() -> std::io::Result<PathBuf> {
    absolute_exe_path().map(|exe| {
        std::env::current_dir()
            .ok()
            .and_then(|wdir| exe.strip_prefix(wdir).ok())
            .map(|p| p.to_path_buf())
            .unwrap_or(exe)
    })
}

/// Initializes the global logger.
///
/// Call this first!
///
/// # Example
///
/// ```
/// use alumet_agent::init_logger;
///
/// /// Runs my new amazing Alumet agent.
/// fn main() {
///     init_logger();
///     log::info!("I can log now!");
/// }
/// ```
pub fn init_logger() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Print a warning if we are running in debug mode.
    #[cfg(debug_assertions)]
    {
        log::warn!("DEBUG assertions are enabled, this build of Alumet is fine for debugging, but not for production.");
    }
}
