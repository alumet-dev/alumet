use std::path::Path;
use crate::config::ContainerRuntime;

/// Extracts the container ID from a cgroup path based on the runtime type.
///
/// # Expected Format
///
/// For Docker:
/// - `/sys/fs/cgroup/docker/<container_id>/...`
/// - `/sys/fs/cgroup/buildkit/<container_id>/...` (buildkit containers)
/// - `/sys/fs/cgroup/system.slice/docker-<container_id>.scope` (systemd cgroups)
///
/// For Podman:
/// - `/sys/fs/cgroup/libpod_parent/<container_id>/...`
/// - `/sys/fs/cgroup/user.slice/libpod_parent/<container_id>/...`
/// - `/sys/fs/cgroup/user.slice/user-<uid>.slice/user-<uid>@.service/libpod-<container_id>.scope` (systemd)
///
/// # Returns
///
/// Returns `Some(container_id)` if the path matches a known pattern,
/// or `None` if the path does not correspond to a container cgroup.
pub fn extract_container_id(cgroup_fs_path: &Path, runtime: ContainerRuntime) -> Option<String> {
    let path_str = cgroup_fs_path.to_str()?;
    
    match runtime {
        ContainerRuntime::Docker => extract_docker_container_id(path_str),
        ContainerRuntime::Podman => extract_podman_container_id(path_str),
    }
}

/// Extracts container ID from Docker cgroup paths
fn extract_docker_container_id(path_str: &str) -> Option<String> {
    // Pattern 1: /sys/fs/cgroup/docker/<container_id>/
    if let Some(id) = extract_between(path_str, "/docker/", "/") {
        return Some(id);
    }
    
    // Pattern 2: /sys/fs/cgroup/buildkit/<container_id>/
    if let Some(id) = extract_between(path_str, "/buildkit/", "/") {
        return Some(id);
    }
    
    // Pattern 3: systemd cgroups: docker-<container_id>.scope
    // Convert "/" pattern to match systemd scopes
    for component in path_str.split('/') {
        if component.starts_with("docker-") && component.ends_with(".scope") {
            let id = component
                .strip_prefix("docker-")?
                .strip_suffix(".scope")?;
            return Some(id.to_string());
        }
    }
    
    None
}

/// Extracts container ID from Podman cgroup paths
fn extract_podman_container_id(path_str: &str) -> Option<String> {
    // Pattern 1: /sys/fs/cgroup/libpod_parent/<container_id>/
    if let Some(id) = extract_between(path_str, "/libpod_parent/", "/") {
        return Some(id);
    }
    
    // Pattern 2: /sys/fs/cgroup/<user_slice>/libpod_parent/<container_id>/
    if let Some(after_libpod) = path_str.split("/libpod_parent/").nth(1) {
        if let Some(id) = after_libpod.split('/').next() {
            return Some(id.to_string());
        }
    }
    
    // Pattern 3: systemd cgroups: libpod-<container_id>.scope
    for component in path_str.split('/') {
        if component.starts_with("libpod-") && component.ends_with(".scope") {
            let id = component
                .strip_prefix("libpod-")?
                .strip_suffix(".scope")?;
            return Some(id.to_string());
        }
    }
    
    // Pattern 4: pod handling: libpod-<pod_id>-<container_id>.scope
    for component in path_str.split('/') {
        if component.starts_with("libpod-") {
            let parts: Vec<&str> = component
                .strip_prefix("libpod-")?
                .strip_suffix(".scope")?
                .split('-')
                .collect();
            
            // We want the last part which is the container ID
            if let Some(container_id) = parts.last() {
                // Only return if it looks like a valid container ID (hex chars, length 12 or 64)
                if is_valid_container_id(container_id) {
                    return Some(container_id.to_string());
                }
            }
        }
    }
    
    None
}

/// Helper function to extract text between two patterns
fn extract_between(text: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = text.find(start)?;
    let after_start = &text[start_idx + start.len()..];
    let end_idx = after_start.find(end)?;
    Some(after_start[..end_idx].to_string())
}

/// Validates if a string looks like a valid container ID
/// Container IDs are hex strings, typically 12 or 64 characters
fn is_valid_container_id(id: &str) -> bool {
    let valid_len = id.len() == 12 || id.len() == 64;
    let is_hex = id.chars().all(|c| c.is_ascii_hexdigit());
    valid_len && is_hex && !id.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_docker_simple_path() {
        let path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_docker_full_path() {
        let path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2/");
        let result = extract_container_id(&path, ContainerRuntime::Docker);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 64);
    }

    #[test]
    fn test_docker_nested_path() {
        let path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6/cpuacct.stat");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_docker_systemd_scope() {
        let path = PathBuf::from("/sys/fs/cgroup/system.slice/docker-a1b2c3d4e5f6.scope/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_podman_simple_path() {
        let path = PathBuf::from("/sys/fs/cgroup/libpod_parent/a1b2c3d4e5f6/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Podman),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_podman_user_slice_path() {
        let path = PathBuf::from("/sys/fs/cgroup/user.slice/user-1000.slice/libpod_parent/a1b2c3d4e5f6/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Podman),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_podman_systemd_scope() {
        let path = PathBuf::from("/sys/fs/cgroup/user.slice/libpod-a1b2c3d4e5f6.scope/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Podman),
            Some("a1b2c3d4e5f6".to_string())
        );
    }

    #[test]
    fn test_non_container_path() {
        let path = PathBuf::from("/sys/fs/cgroup/system.slice/apache2.service/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            None
        );
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Podman),
            None
        );
    }

    #[test]
    fn test_empty_path() {
        let path = PathBuf::from("");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            None
        );
    }

    #[test]
    fn test_mismatched_runtime() {
        let docker_path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6/");
        assert_eq!(
            extract_container_id(&docker_path, ContainerRuntime::Podman),
            None
        );

        let podman_path = PathBuf::from("/sys/fs/cgroup/libpod_parent/a1b2c3d4e5f6/");
        assert_eq!(
            extract_container_id(&podman_path, ContainerRuntime::Docker),
            None
        );
    }

    #[test]
    fn test_buildkit_path() {
        let path = PathBuf::from("/sys/fs/cgroup/buildkit/buildx-container/");
        assert_eq!(
            extract_container_id(&path, ContainerRuntime::Docker),
            Some("buildx-container".to_string())
        );
    }

    #[test]
    fn test_valid_container_id() {
        assert!(is_valid_container_id("a1b2c3d4e5f6"));
        assert!(is_valid_container_id("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"));
        assert!(!is_valid_container_id("abc"));
        assert!(!is_valid_container_id("ghijklmnop"));
        assert!(!is_valid_container_id(""));
    }
}