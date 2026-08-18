use std::time::Duration;
use serde::{Deserialize, Serialize};

/// Container runtime type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerRuntime {
    Docker,
    Podman,
}

impl ContainerRuntime {
    /// Returns default Unix socket path for each runtime
    pub fn default_unix_socket(&self) -> &str {
        match self {
            ContainerRuntime::Docker => "/var/run/docker.sock",
            ContainerRuntime::Podman => "/run/podman/podman.sock",
        }
    }
    
    /// Returns default API URL for each runtime
    pub fn default_url(&self) -> String {
        self.default_unix_socket_with_prefix()
    }

    /// Returns URL with unix:// prefix
    pub fn default_unix_socket_with_prefix(&self) -> String {
        format!("unix://{}", self.default_unix_socket())
    }
    
    pub fn name(&self) -> &str {
        match self {
            ContainerRuntime::Docker => "docker",
            ContainerRuntime::Podman => "podman",
        }
    }
}

impl std::fmt::Display for ContainerRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Plugin configuration
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// Container runtime to use
    pub runtime: ContainerRuntime,
    
    /// URL to the container API (supports both http:// and unix:// URLs)
    /// 
    /// Examples:
    /// - unix:///var/run/docker.sock (Docker default)
    /// - unix:////wsl.localhost/docker-desktop-data/data/docker-desktop-root-certs/docker.sock (WSL2 Docker)
    /// - unix:///run/podman/podman.sock (Podman daemon)
    /// - unix:///run/user/1000/podman/podman.sock (Podman rootless)
    /// - http://localhost:2375 (Docker over TCP)
    #[serde(default = "default_api_url")]
    pub api_url: Option<String>,
    
    /// Optional: explicit Unix socket path (alternative to api_url for unix sockets)
    /// If specified, this will be used instead of deriving from api_url
    /// Example: "/var/run/docker.sock" or "/run/podman/podman.sock"
    #[serde(default)]
    pub socket_path: Option<String>,
    
    /// Optional: override for automatic detection of WSL2 socket path
    /// When running in WSL2, set this to false to disable automatic WSL socket detection
    #[serde(default = "default_detect_wsl")]
    pub detect_wsl: bool,
    
    /// Optional: override automatic pipe detection (Windows named pipes for Docker Desktop)
    /// Not commonly used, but available for Windows-specific setups
    #[serde(default)]
    pub use_windows_pipe: bool,
    
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,
    
    /// If `true`, adds attributes like `container_id`, `container_name`, `container_image`, `runtime` 
    /// to the cgroup measurements produced by other plugins.
    #[serde(default)]
    pub annotate_foreign_measurements: bool,
    
    /// If `true`, includes container labels as attributes (prefixed with `label.`)
    #[serde(default)]
    pub include_container_labels: bool,
}

fn default_api_url() -> Option<String> {
    None
}

fn default_detect_wsl() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runtime: ContainerRuntime::Docker,
            api_url: None,
            socket_path: None,
            detect_wsl: true,
            use_windows_pipe: false,
            poll_interval: Duration::from_secs(5),
            annotate_foreign_measurements: false,
            include_container_labels: false,
        }
    }
}

impl Config {
    pub fn api_url(&self) -> String {
        self.api_url.clone().unwrap_or_else(|| self.runtime.default_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_runtime_default_unix_socket() {
        assert_eq!(ContainerRuntime::Docker.default_unix_socket(), "/var/run/docker.sock");
        assert_eq!(ContainerRuntime::Podman.default_unix_socket(), "/run/podman/podman.sock");
    }
    
    #[test]
    fn test_runtime_default_url() {
        assert_eq!(ContainerRuntime::Docker.default_url(), "unix:///var/run/docker.sock");
        assert_eq!(ContainerRuntime::Podman.default_url(), "unix:///run/podman/podman.sock");
    }
    
    #[test]
    fn test_runtime_name() {
        assert_eq!(ContainerRuntime::Docker.name(), "docker");
        assert_eq!(ContainerRuntime::Podman.name(), "podman");
    }
    
    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.runtime, ContainerRuntime::Docker);
        assert_eq!(config.api_url, None);
        assert_eq!(config.socket_path, None);
        assert_eq!(config.detect_wsl, true);
        assert_eq!(config.use_windows_pipe, false);
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert!(!config.annotate_foreign_measurements);
        assert!(!config.include_container_labels);
    }
    
    #[test]
    fn test_config_api_url() {
        let mut config = Config::default();
        assert_eq!(config.api_url(), "unix:///var/run/docker.sock");
        
        config.api_url = Some("http://localhost:8080".to_string());
        assert_eq!(config.api_url(), "http://localhost:8080");
        
        config.api_url = Some("unix:///custom/path/docker.sock".to_string());
        assert_eq!(config.api_url(), "unix:///custom/path/docker.sock");
    }
    
    #[test]
    fn test_runtime_display() {
        assert_eq!(ContainerRuntime::Docker.to_string(), "docker");
        assert_eq!(ContainerRuntime::Podman.to_string(), "podman");
    }
}