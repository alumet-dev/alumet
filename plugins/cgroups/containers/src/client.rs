use std::path::Path;
use std::sync::atomic::AtomicUsize;
use crate::config::ContainerRuntime;
use anyhow::Context;
use log::info;
use bollard::{
    Docker,
    container::{ListContainersOptions},
    models::{ContainerSummary as BollardContainerSummary, ContainerInspectResponse},
};
use bollard::ClientVersion;

/// Container information extracted from the API
#[derive(Debug, Clone)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub image: String,
    pub runtime: ContainerRuntime,
    pub labels: Vec<(String, String)>,
    pub created: Option<i64>,
}

/// HTTP client for Docker and Podman APIs using bollard
#[derive(Clone)]
pub struct ApiClient {
    runtime: ContainerRuntime,
    docker: Docker,
    api_url: String,
}

impl ApiClient {
    pub fn new(runtime: ContainerRuntime, api_url: &str) -> anyhow::Result<Self> {
        let (effective_url, socket_path) = Self::parse_socket_url(api_url);
        
        info!("Initialized {} API client with URL: {}", runtime, effective_url);
        if let Some(ref socket) = socket_path {
            info!("Using Unix socket at: {}", socket);
        }

        // Create Docker client using bollard with a specific API version
        let major = AtomicUsize::new(1);
        let minor = AtomicUsize::new(41);
        let client_version = ClientVersion::from(&(major, minor));
        let docker = if let Some(socket_path) = socket_path {
            // Use Unix socket connection
            Docker::connect_with_socket(&socket_path, 120, &client_version)
                .context(format!("failed to connect to Unix socket at {}", socket_path))?
        } else {
            // Use HTTP connection
            Docker::connect_with_http(effective_url.as_str(), 120, &client_version)
                .context(format!("failed to connect to HTTP endpoint at {}", effective_url))?
        };

        Ok(Self {
            runtime,
            docker,
            api_url: effective_url,
        })
    }

    /// Parse URL and extract Unix socket path if present
    fn parse_socket_url(url: &str) -> (String, Option<String>) {
        if url.starts_with("unix://") {
            (url.to_string(), Some(url.strip_prefix("unix://").unwrap().to_string()))
        } else {
            (url.to_string(), None)
        }
    }

    /// Lists all containers (including stopped ones)
    pub async fn list_containers(&self) -> anyhow::Result<Vec<ContainerInfo>> {
        // Create options to list all containers including stopped ones
        let options = Some(ListContainersOptions::<String> {
            all: true,
            ..Default::default()
        });

        // Use bollard to list containers
        let bollard_containers = self.docker
            .list_containers(options)
            .await
            .context("failed to list containers from API")?;

        // Convert bollard types to our ContainerInfo
        let containers = bollard_containers
            .into_iter()
            .map(|c| {
                let mut info = ContainerInfo::from(c);
                info.runtime = self.runtime;
                info
            })
            .collect();

        Ok(containers)
    }

    /// Lists all containers (blocking wrapper for async)
    pub fn list_containers_blocking(&self) -> anyhow::Result<Vec<ContainerInfo>> {
        // Create a runtime for blocking execution
        let rt = tokio::runtime::Runtime::new()
            .context("failed to create async runtime")?;
        
        rt.block_on(self.list_containers())
    }

    /// Inspects a specific container by ID
    pub async fn inspect_container(&self, id: &str) -> anyhow::Result<ContainerInfo> {
        let inspect_response = self.docker
            .inspect_container(id, None)
            .await
            .context(format!("failed to inspect container {}", id))?;

        self.convert_inspect_response(inspect_response)
    }

    /// Inspects a specific container by ID (blocking wrapper for async)
    pub fn inspect_container_blocking(&self, id: &str) -> anyhow::Result<ContainerInfo> {
        let rt = tokio::runtime::Runtime::new()
            .context("failed to create async runtime")?;
        
        rt.block_on(self.inspect_container(id))
    }

    /// Convert bollard inspect response to ContainerInfo
    fn convert_inspect_response(&self, inspect: ContainerInspectResponse) -> anyhow::Result<ContainerInfo> {
        let id = inspect.id.ok_or_else(|| anyhow::anyhow!("Container ID missing in inspect response"))?;
        
        let name = inspect.name
            .as_ref()
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.clone());

        let (image, labels) = if let Some(config) = inspect.config {
            let img = config.image.unwrap_or_else(|| "unknown".to_string());
            let lbls = config.labels.unwrap_or_default().into_iter().collect();
            (img, lbls)
        } else {
            ("unknown".to_string(), Vec::new())
        };

        let created = inspect.created
            .and_then(|c| c.parse::<i64>().ok());

        Ok(ContainerInfo {
            id,
            name,
            image,
            runtime: self.runtime,
            labels,
            created,
        })
    }

    pub fn runtime(&self) -> ContainerRuntime {
        self.runtime
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

impl From<BollardContainerSummary> for ContainerInfo {
    fn from(summary: BollardContainerSummary) -> Self {
        let id = summary.id.unwrap_or_else(|| "unknown".to_string());
        
        let name = if let Some(names) = &summary.names {
            if let Some(first_name) = names.first() {
                first_name.trim_start_matches('/').to_string()
            } else {
                id.clone()
            }
        } else {
            id.clone()
        };

        let image = summary.image.unwrap_or_else(|| "unknown".to_string());

        let labels = summary.labels
            .unwrap_or_default()
            .into_iter()
            .collect();

        let created = summary.created;

        ContainerInfo {
            id,
            name,
            image,
            runtime: ContainerRuntime::Docker, // Will be set by the client
            labels,
            created,
        }
    }
}

/// Helper function to detect WSL2 environment and return appropriate socket path
pub fn detect_wsl_socket_path(runtime: ContainerRuntime) -> Option<String> {
    // Check if running under WSL by examining /proc/version
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        if version.contains("Microsoft") || version.contains("microsoft") {
            info!("WSL2 environment detected");
            
            // Try WSL2-specific paths first
            let wsl_paths = match runtime {
                ContainerRuntime::Docker => vec![
                    // Primary Windows-hosted Docker Desktop socket via WSL2 localhost
                    "//wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock",
                    // Alternative Windows-hosted socket path
                    "//wsl.localhost/docker-desktop/run/docker.sock",
                    // Legacy WSL mount path (may not work reliably)
                    "/mnt/wsl/docker-desktop/run/docker.sock",
                    // Mapped Windows socket path
                    "/mnt/c/Users/Public/docker.sock",
                    // Standard Linux socket (fallback, may not work in WSL)
                    "/var/run/docker.sock",
                ],
                ContainerRuntime::Podman => vec![
                    "/run/podman/podman.sock", // Standard path usually works in WSL
                    "/var/run/podman/podman.sock", // Alternative standard path
                ],
            };

            for path in wsl_paths {
                if Path::new(path).exists() {
                    info!("Found socket at WSL path: {}", path);
                    return Some(format!("unix://{}", path));
                }
            }
            
            info!("No socket found in WSL2 environment, please specify api_url explicitly");
        }
    }
    None
}

/// Helper function to detect Podman daemon mode
pub fn detect_podman_daemon_socket() -> Option<String> {
    let daemon_paths = vec![
        "/run/podman/podman.sock",    // Standard daemon socket
        "/var/run/podman/podman.sock", // Alternative standard path
        "/run/user/1000/podman/podman.sock", // User namespace daemon
    ];

    for path in daemon_paths {
        if Path::new(path).exists() {
            info!("Found Podman daemon socket at: {}", path);
            return Some(format!("unix://{}", path));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_socket_url_unix() {
        let (_http_url, socket_path) = ApiClient::parse_socket_url("unix:///var/run/docker.sock");
        assert_eq!(socket_path, Some("/var/run/docker.sock".to_string()));
    }

    #[test]
    fn test_parse_socket_url_http() {
        let (http_url, socket_path) = ApiClient::parse_socket_url("http://localhost:8080");
        assert_eq!(http_url, "http://localhost:8080".to_string());
        assert!(socket_path.is_none());
    }
}