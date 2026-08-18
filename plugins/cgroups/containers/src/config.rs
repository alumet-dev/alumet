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
    pub fn default_url(&self) -> String {
        match self {
            ContainerRuntime::Docker => "unix:///var/run/docker.sock".to_string(),
            ContainerRuntime::Podman => "unix:///run/podman/podman.sock".to_string(),
        }
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
    
    /// URL to the container API
    #[serde(default = "default_api_url")]
    pub api_url: Option<String>,
    
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

impl Default for Config {
    fn default() -> Self {
        Self {
            runtime: ContainerRuntime::Docker,
            api_url: None,
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
    }
    
    #[test]
    fn test_runtime_display() {
        assert_eq!(ContainerRuntime::Docker.to_string(), "docker");
        assert_eq!(ContainerRuntime::Podman.to_string(), "podman");
    }
}