use std::path::PathBuf;

use anyhow::anyhow;

pub mod run;
pub mod tests;

/// Returns an empty directory created in the system temp directory ([`std::env::temp_dir`])
/// or in a subfolder of it.
///
/// # Errors
/// If the directory cannot be emptied or created, returns an error.
pub fn empty_temp_dir(key: &str) -> anyhow::Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("alumet-app-agent-tests/{}-{key}", env!("CARGO_CRATE_NAME")));
    match std::fs::remove_dir_all(&dir) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!("failed to remove dir {dir:?}: {e}")),
    }?;
    match std::fs::create_dir_all(&dir) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(anyhow!("failed to create dir {dir:?}: {e}")),
    }?;
    Ok(dir)
}
