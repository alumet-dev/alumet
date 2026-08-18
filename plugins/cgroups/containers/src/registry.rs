use alumet::measurement::AttributeValue;
use anyhow::Context;
use rustc_hash::FxHashMap;
use util_cgroups_plugins::job_annotation_transform::JobTagger;
use util_cgroups::Cgroup;

use crate::client::{ApiClient, ContainerInfo};
use crate::extraction::extract_container_id;
use crate::config::Config;

/// Automatically-refreshed container registry.
/// Keeps track of containers and their metadata.
#[derive(Clone)]
pub struct ContainerRegistry {
    client: ApiClient,
    config: Config,
    pub(crate) containers: FxHashMap<String, ContainerInfo>,
}

impl ContainerRegistry {
    pub fn new(client: ApiClient, config: Config) -> Self {
        Self {
            client,
            config,
            containers: FxHashMap::default(),
        }
    }

    /// Refreshes the container list by querying the API
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        log::debug!("Refreshing container list for {}", self.client.runtime());
        
        let all_containers = self.client.list_containers_blocking()
            .context("failed to list containers")?;
        
        self.containers = all_containers
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();
        
        log::debug!("Loaded {} containers", self.containers.len());
        Ok(())
    }

    /// Gets information about a specific container by ID.
    /// If the container is not in the cache, it will refresh the cache and try again.
    pub fn get(&mut self, container_id: &str) -> anyhow::Result<Option<ContainerInfo>> {
        // Check cache first
        if let Some(info) = self.containers.get(container_id) {
            return Ok(Some(info.clone()));
        }

        // Not in cache, refresh and try again
        log::debug!("Container {} not in cache, refreshing", container_id);
        self.refresh()?;

        match self.containers.get(container_id) {
            Some(info) => Ok(Some(info.clone())),
            None => {
                // Container might have been deleted
                log::trace!("Container {} not found after refresh", container_id);
                Ok(None)
            }
        }
    }

    /// Returns the runtime type
    pub fn runtime(&self) -> crate::config::ContainerRuntime {
        self.client.runtime()
    }
}

impl JobTagger for ContainerRegistry {
    fn attributes_for_cgroup(&mut self, cgroup: &Cgroup) -> Vec<(String, AttributeValue)> {
        let container_id = match extract_container_id(cgroup.fs_path(), self.runtime()) {
            Some(id) => id,
            None => return Vec::new(),
        };

        let runtime = self.runtime();
        let container_info = match self.get(&container_id) {
            Ok(Some(info)) => info,
            Ok(None) => {
                log::trace!("No info found for container {}", container_id);
                return Vec::new();
            }
            Err(e) => {
                log::error!("Failed to get container info for {}: {}", container_id, e);
                return Vec::new();
            }
        };

        let runtime_name = runtime.name();
        
        let mut attributes = vec![
            ("container_id".to_string(), AttributeValue::String(container_info.id.clone())),
            ("container_name".to_string(), AttributeValue::String(container_info.name)),
            ("container_image".to_string(), AttributeValue::String(container_info.image)),
            ("runtime".to_string(), AttributeValue::String(runtime_name.to_string())),
        ];

        // Add container labels if enabled
        if self.config.include_container_labels {
            for (key, value) in container_info.labels {
                let attr_key = format!("label.{}", key);
                attributes.push((attr_key, AttributeValue::String(value)));
            }
        }

        // Add creation timestamp if available
        if let Some(created) = container_info.created {
            attributes.push(("container_created".to_string(), AttributeValue::U64(created as u64)));
        }

        attributes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, ContainerRuntime};
    use std::path::PathBuf;

    fn create_mock_container_info(id: &str, name: &str, image: &str) -> ContainerInfo {
        ContainerInfo {
            id: id.to_string(),
            name: name.to_string(),
            image: image.to_string(),
            runtime: ContainerRuntime::Docker,
            labels: vec![("key1".to_string(), "value1".to_string())],
            created: Some(1234567890),
        }
    }

    fn create_test_registry() -> (ContainerRegistry, Config) {
        let config = Config::default();
        // Note: In real tests, we would use a mock API client
        // For now, we'll create a dummy client that won't actually connect
        let client = ApiClient::new(ContainerRuntime::Docker, "http://localhost:8080")
            .unwrap();
        let mut registry = ContainerRegistry::new(client, config);
        
        // Manually add some containers for testing
        registry.containers.insert(
            "a1b2c3d4e5f6".to_string(),
            create_mock_container_info("a1b2c3d4e5f6", "test-container", "ubuntu:latest"),
        );
        
        registry.containers.insert(
            "f6e5d4c3b2a1".to_string(),
            create_mock_container_info("f6e5d4c3b2a1", "another-container", "nginx:latest"),
        );
        
        let config = Config::default();
        (registry, config)
    }

    #[test]
    fn test_registry_get_existing() {
        let (mut registry, _) = create_test_registry();
        
        let result = registry.get("a1b2c3d4e5f6").unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.name, "test-container");
    }

    #[test]
    fn test_registry_get_nonexistent() {
        let (mut registry, _) = create_test_registry();
        
        let result = registry.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_attributes_for_cgroup_docker() {
        let (mut registry, mut config) = create_test_registry();
        config.include_container_labels = true;
        registry.config = config.clone();
        
        // Create a mock cgroup for testing
        use util_cgroups::{CgroupHierarchy, CgroupVersion};
        let hierarchy = CgroupHierarchy::manually_unchecked(
            "/sys/fs/cgroup",
            CgroupVersion::V2,
            vec!["cpu", "memory"]
        );
        let cgroup = Cgroup::from_cgroup_path(&hierarchy, "/docker/a1b2c3d4e5f6".to_string());
        
        let attrs = registry.attributes_for_cgroup(&cgroup);
        
        assert!(!attrs.is_empty());
        
        let attrs_map: std::collections::HashMap<_, _> = attrs.into_iter().collect();
        
        assert_eq!(
            attrs_map.get("container_id"),
            Some(&AttributeValue::String("a1b2c3d4e5f6".to_string()))
        );
        assert_eq!(
            attrs_map.get("container_name"),
            Some(&AttributeValue::String("test-container".to_string()))
        );
        assert_eq!(
            attrs_map.get("container_image"),
            Some(&AttributeValue::String("ubuntu:latest".to_string()))
        );
        assert_eq!(
            attrs_map.get("runtime"),
            Some(&AttributeValue::String("docker".to_string()))
        );
        assert_eq!(
            attrs_map.get("label.key1"),
            Some(&AttributeValue::String("value1".to_string()))
        );
    }

    #[test]
    fn test_attributes_for_cgroup_no_labels() {
        let (mut registry, mut config) = create_test_registry();
        config.include_container_labels = false;
        registry.config = config.clone();
        
        // Create a mock cgroup for testing
        use util_cgroups::{CgroupHierarchy, CgroupVersion};
        let hierarchy = CgroupHierarchy::manually_unchecked(
            "/sys/fs/cgroup",
            CgroupVersion::V2,
            vec!["cpu", "memory"]
        );
        let cgroup = Cgroup::from_cgroup_path(&hierarchy, "/docker/a1b2c3d4e5f6".to_string());
        
        let attrs = registry.attributes_for_cgroup(&cgroup);
        
        let attrs_map: std::collections::HashMap<_, _> = attrs.into_iter().collect();
        
        assert!(attrs_map.contains_key("container_id"));
        assert!(attrs_map.contains_key("container_name"));
        assert!(attrs_map.contains_key("container_image"));
        assert!(attrs_map.contains_key("runtime"));
        assert!(!attrs_map.contains_key("label.key1"));
    }

    #[test]
    fn test_attributes_for_cgroup_non_container() {
        let (mut registry, _) = create_test_registry();
        
        // Create a mock cgroup that's not a container
        use util_cgroups::{CgroupHierarchy, CgroupVersion};
        let hierarchy = CgroupHierarchy::manually_unchecked(
            "/sys/fs/cgroup",
            CgroupVersion::V2,
            vec!["cpu", "memory"]
        );
        let cgroup = Cgroup::from_cgroup_path(&hierarchy, "/system.slice/apache2.service".to_string());
        
        let attrs = registry.attributes_for_cgroup(&cgroup);
        assert!(attrs.is_empty());
    }
}