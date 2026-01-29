use std::{io::ErrorKind, path::Path};

use tempfile::TempDir;

pub mod run;
pub mod tests;

#[derive(Debug)]
pub struct TestDir(pub TempDir);

/// Returns an empty directory created in the system temp directory ([`std::env::temp_dir`])
/// or in a subfolder of it.
///
/// # Errors
/// If the directory cannot be emptied or created, returns an error.
pub fn empty_temp_dir() -> anyhow::Result<TestDir> {
    let parent = std::env::temp_dir().join(format!("alumet-{}", env!("CARGO_CRATE_NAME")));
    match std::fs::create_dir(&parent) {
        Ok(_) => (),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => (),
        Err(err) => return Err(err.into()),
    };
    let dir = tempfile::tempdir_in(parent)?;
    Ok(TestDir(dir))
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Keep the directory, to make it easier to debug the test outputs.
            self.0.disable_cleanup(true);
        }
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}
