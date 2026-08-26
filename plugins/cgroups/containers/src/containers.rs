use std::path::Path;
use std::time::Duration;

use alumet::measurement::AttributeValue;
use anyhow::Context;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use util_cgroups::Cgroup;
use util_cgroups_plugins::job_annotation_transform::JobTagger;

/// Plugin configuration
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// If `true`, adds attributes like `uid`, `name` to the cgroup measurements
    /// produced by other plugins.
    #[serde(default)]
    pub annotate_foreign_measurements: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            annotate_foreign_measurements: false,
        }
    }
}

// Info that will be added to the metrics
#[derive(Debug, Clone)]
pub struct ContainerInfos {
    pub uid: String,
    pub name: String,
}

/// Extracts the container UID from a cgroup path by trying OCI runtime patterns in sequence.
/// Returns `Some(container_uid)` if the path matches any known pattern,
/// or `None` if the path does not correspond to a container cgroup.
pub fn extract_container_uid(cgroup_fs_path: &Path) -> Option<String> {
    let path_str = cgroup_fs_path.to_str()?;

    // Try Podman patterns
    if let Some(uid) = extract_podman_container_uid(path_str) {
        return Some(uid);
    }

    // Fall back to Docker patterns
    extract_docker_container_uid(path_str)
}

/// Extracts container UID from Docker cgroup paths
/// - `/sys/fs/cgroup/docker/<container_uid>/...`
/// - `/sys/fs/cgroup/buildkit/<container_uid>/...` (buildkit containers)
/// - `/sys/fs/cgroup/system.slice/docker-<container_uid>.scope` (systemd cgroups)
fn extract_docker_container_uid(path_str: &str) -> Option<String> {
    if let Some(uid) = extract_between(path_str, "/docker/", "/") {
        return Some(uid);
    }

    if let Some(uid) = extract_between(path_str, "/buildkit/", "/") {
        return Some(uid);
    }

    for component in path_str.split('/') {
        if component.starts_with("docker-") && component.ends_with(".scope") {
            let uid = component.strip_prefix("docker-")?.strip_suffix(".scope")?;
            return Some(uid.to_string());
        }
    }

    None
}

/// Extracts container UID from Podman cgroup paths
/// - `/sys/fs/cgroup/libpod_parent/<container_uid>/...`
/// - `/sys/fs/cgroup/user.slice/libpod_parent/<container_uid>/...`
/// - `/sys/fs/cgroup/user.slice/user-<uid>.slice/user-<uid>@.service/libpod-<container_uid>.scope` (systemd)
fn extract_podman_container_uid(path_str: &str) -> Option<String> {
    if let Some(uid) = extract_between(path_str, "/libpod_parent/", "/") {
        return Some(uid);
    }

    if let Some(after_libpod) = path_str.split("/libpod_parent/").nth(1) {
        if let Some(uid) = after_libpod.split('/').next() {
            return Some(uid.to_string());
        }
    }

    for component in path_str.split('/') {
        if component.starts_with("libpod-") && component.ends_with(".scope") {
            let uid = component.strip_prefix("libpod-")?.strip_suffix(".scope")?;
            return Some(uid.to_string());
        }
    }

    for component in path_str.split('/') {
        if component.starts_with("libpod-") {
            let parts: Vec<&str> = component
                .strip_prefix("libpod-")?
                .strip_suffix(".scope")?
                .split('-')
                .collect();

            return Some(parts.last()?.to_string());
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

/// HTTP client for OCI APIs using bollard
#[derive(Clone)]
pub struct ApiClient {
    docker: bollard::Docker,
}

impl ApiClient {
    pub fn new() -> anyhow::Result<Self> {
        let docker = Self::try_connect_with_fallback()
            .context("failed to connect to any container runtime (Docker or Podman)")?;
        Ok(Self { docker })
    }

    fn try_connect_with_fallback() -> anyhow::Result<bollard::Docker> {
        let rt = tokio::runtime::Runtime::new()
            .context("failed to create async runtime for connection testing")?;

        // Try Docker first
        log::debug!("Attempting to connect to Docker...");
        if let Ok(docker) = bollard::Docker::connect_with_unix_defaults() {
            // Ping to verify the connection is actually working
            if rt.block_on(Self::test_connection(&docker)) {
                log::info!("Successfully connected to Docker (ping successful)");
                return Ok(docker);
            } else {
                log::debug!("Docker socket found but ping failed");
            }
        }

        // Docker failed, try Podman
        log::debug!("Attempting to connect to Podman...");
        if let Ok(docker) = bollard::Docker::connect_with_podman_defaults() {
            // Ping to verify the connection is actually working
            if rt.block_on(Self::test_connection(&docker)) {
                log::info!("Successfully connected to Podman (ping successful)");
                return Ok(docker);
            } else {
                log::debug!("Podman socket found but ping failed");
            }
        }

        // Try additional Podman socket locations manually
        log::debug!("Attempting to connect to Podman via additional socket locations...");
        
        let socket_paths = vec![
            "/run/user/1094/podman/podman.sock",  // User ID 1094 (from your podman info)
            "/run/user/1000/podman/podman.sock",   // Common user ID
            "/run/podman/podman.sock",              // System-wide
            "/var/run/podman/podman.sock",          // Alternative system-wide
        ];

        for socket_path in socket_paths {
            if Path::new(socket_path).exists() {
                log::debug!("Found Podman socket at: {}", socket_path);
                match bollard::Docker::connect_with_unix(
                    socket_path,
                    60, // timeout in seconds
                    bollard::API_DEFAULT_VERSION
                ) {
                    Ok(docker) => {
                        if rt.block_on(Self::test_connection(&docker)) {
                            log::info!("Successfully connected to Podman via {} (ping successful)", socket_path);
                            return Ok(docker);
                        } else {
                            log::debug!("Podman socket at {} found but ping failed", socket_path);
                        }
                    }
                    Err(e) => {
                        log::debug!("Failed to connect to Podman socket {}: {}", socket_path, e);
                    }
                }
            } else {
                log::debug!("Podman socket not found at: {}", socket_path);
            }
        }

        // Try connecting to Podman via environment variable if set
        if let Ok(socket_path) = std::env::var("PODMAN_SOCKET") {
            log::debug!("Trying Podman socket from PODMAN_SOCKET environment variable: {}", socket_path);
            if Path::new(&socket_path).exists() {
                match bollard::Docker::connect_with_unix(
                    &socket_path,
                    60,
                    bollard::API_DEFAULT_VERSION
                ) {
                    Ok(docker) => {
                        if rt.block_on(Self::test_connection(&docker)) {
                            log::info!("Successfully connected to Podman via PODMAN_SOCKET (ping successful)");
                            return Ok(docker);
                        }
                    }
                    Err(e) => {
                        log::debug!("Failed to connect via PODMAN_SOCKET: {}", e);
                    }
                }
            }
        }

        // Both failed, return comprehensive error
        Err(anyhow::anyhow!(
            "Could not connect to any container runtime. \
             Tried Docker (unix defaults) and Podman at multiple locations. \
             Please ensure either Docker daemon or Podman service is running. \
             For Podman, you may need to: 1) Start the service: 'systemctl --user start podman.socket' \
             2) Set the PODMAN_SOCKET environment variable to the correct socket path."
        ))
    }

    async fn test_connection(docker: &bollard::Docker) -> bool {
        match docker.ping().await {
            Ok(_) => true,
            Err(e) => {
                log::debug!("Connection ping failed: {}", e);
                false
            }
        }
    }

    /// Lists all containers (including stopped ones)
    pub async fn list_containers(&self) -> anyhow::Result<Vec<ContainerInfos>> {
        let options = Some(bollard::query_parameters::ListContainersOptions {
            all: true,
            ..Default::default()
        });

        let bollard_containers = self
            .docker
            .list_containers(options)
            .await
            .context("failed to list containers from API")?;

        // Convert bollard types to our ContainerInfos
        let containers = bollard_containers
            .into_iter()
            .map(|c| ContainerInfos::from(c))
            .collect();

        Ok(containers)
    }

    /// Lists all containers (blocking wrapper for async)
    pub fn list_containers_blocking(&self) -> anyhow::Result<Vec<ContainerInfos>> {
        let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;

        rt.block_on(self.list_containers())
    }
}

impl From<bollard::models::ContainerSummary> for ContainerInfos {
    fn from(summary: bollard::models::ContainerSummary) -> Self {
        let uid = summary.id.unwrap_or_else(|| "unknown".to_string());

        let name = if let Some(names) = &summary.names {
            if let Some(first_name) = names.first() {
                first_name.trim_start_matches('/').to_string()
            } else {
                uid.clone()
            }
        } else {
            uid.clone()
        };

        ContainerInfos { uid, name }
    }
}

/// Automatically-refreshed container registry.
/// Keeps track of containers and their metadata.
#[derive(Clone)]
pub struct AutoContainerRegistry {
    client: ApiClient,
    pub(crate) containers: FxHashMap<String, ContainerInfos>,
}

impl AutoContainerRegistry {
    pub fn new(api_client: ApiClient) -> Self {
        Self {
            client: api_client,
            containers: Default::default(),
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let all_containers = self
            .client
            .list_containers_blocking()
            .context("failed to list containers")?;

        self.containers = all_containers.into_iter().map(|c| (c.uid.clone(), c)).collect();

        Ok(())
    }

    pub fn get(&mut self, container_uid: &str) -> anyhow::Result<Option<ContainerInfos>> {
        if let Some(infos) = self.containers.get(container_uid) {
            return Ok(Some(infos.to_owned()));
        }

        // We have no info about this container
        self.refresh()?;

        match self.containers.get(container_uid) {
            Some(infos) => Ok(Some(infos.to_owned())),
            // It must have been deleted in the meantime
            None => Ok(None),
        }
    }
}

impl JobTagger for AutoContainerRegistry {
    fn attributes_for_cgroup(&mut self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)> {
        let Some(container_uid) = extract_container_uid(cgroup.fs_path()) else {
            return Vec::new();
        };

        match self.get(&container_uid) {
            Ok(Some(container_infos)) => vec![
                ("uid".into(), AttributeValue::String(container_infos.uid)),
                ("name".into(), AttributeValue::String(container_infos.name)),
            ],
            Ok(None) => Vec::new(),
            Err(e) => {
                log::error!("failed to get container info for {container_uid}: {e:#}");
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert!(!config.annotate_foreign_measurements);
    }

    // Docker extraction tests
    #[test]
    fn test_docker_simple_path() {
        let path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    #[test]
    fn test_docker_full_path() {
        let path =
            PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2/");
        let result = extract_container_uid(&path);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 64);
    }

    #[test]
    fn test_docker_nested_path() {
        let path = PathBuf::from("/sys/fs/cgroup/docker/a1b2c3d4e5f6/cpuacct.stat");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    #[test]
    fn test_docker_systemd_scope() {
        let path = PathBuf::from("/sys/fs/cgroup/system.slice/docker-a1b2c3d4e5f6.scope/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    #[test]
    fn test_docker_buildkit_path() {
        let path = PathBuf::from("/sys/fs/cgroup/buildkit/a1b2c3d4e5f6/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    // Podman extraction tests
    #[test]
    fn test_podman_simple_path() {
        let path = PathBuf::from("/sys/fs/cgroup/libpod_parent/a1b2c3d4e5f6/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    #[test]
    fn test_podman_user_slice_path() {
        let path = PathBuf::from("/sys/fs/cgroup/user.slice/user-1000.slice/libpod_parent/a1b2c3d4e5f6/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    #[test]
    fn test_podman_systemd_scope() {
        let path = PathBuf::from("/sys/fs/cgroup/user.slice/libpod-a1b2c3d4e5f6.scope/");
        assert_eq!(extract_container_uid(&path), Some("a1b2c3d4e5f6".to_string()));
    }

    // Not supported path tests
    #[test]
    fn test_non_container_path() {
        let path = PathBuf::from("/sys/fs/cgroup/system.slice/apache2.service/");
        assert_eq!(extract_container_uid(&path), None);
    }
}
