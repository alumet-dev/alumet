mod cgroupv2;
mod k8s;
mod oar3;

use std::path::Path;

pub use k8s::plugin::K8sPlugin;
pub use oar3::plugin::OARPlugin;

/// Check if a specific file is a dir. Used to know if cgroup v2 are used.
///
/// # Return value
///
/// Returns `Ok(true)` if it can be verified that `path` is a directory, and `Ok(false)` if it can be verified that it is not a directory.
/// Returns an error if the path metadata cannot be obtained.
pub fn is_accessible_dir(path: &Path) -> Result<bool, std::io::Error> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use crate::is_accessible_dir;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    // Tests `is_accessible_dir` function to check the existence of cgroupv2 file system
    #[test]
    fn test_is_accessible_dir() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/kubepods-list-metrics");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("dir_cgroup");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(is_accessible_dir(&dir).unwrap());

        let non_existent_path = root.join("non_existent_dir");
        assert!(!is_accessible_dir(&non_existent_path).unwrap());

        let file_path = root.join("data.stat");
        std::fs::write(&file_path, "test file").unwrap();
        assert!(!is_accessible_dir(&file_path).unwrap());
        assert!(!is_accessible_dir(&PathBuf::new()).unwrap());
    }

    // Tests `is_accessible_dir` function with permission error
    #[test]
    fn test_is_accessible_dir_permission_error() {
        let root = Path::new("/root/protected");
        let result = is_accessible_dir(&root);
        assert!(result.is_err());
        if let Err(e) = result {
            assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        }
    }
}
