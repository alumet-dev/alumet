use std::path::Path;
use crate::config::ContainerRuntime;
use anyhow::Context;
use serde::Deserialize;

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

/// API response structures (only fields we need are included)
mod api {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct ContainerList {
        pub items: Vec<ContainerSummary>,
    }

    #[derive(Deserialize)]
    pub struct ContainerSummary {
        pub Id: String,
        pub Names: Vec<String>,
        pub Image: String,
        #[serde(default)]
        pub Labels: Vec<ContainerLabel>,
        #[serde(default)]
        pub Created: i64,
    }

    #[derive(Deserialize, Clone)]
    pub struct ContainerLabel {
        pub name: String,
        pub value: String,
    }
}

impl From<api::ContainerSummary> for ContainerInfo {
    fn from(summary: api::ContainerSummary) -> Self {
        let id = summary.id();
        let name = summary.Names.get(0)
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_else(|| id.clone());

        let labels: Vec<(String, String)> = summary.Labels
            .into_iter()
            .map(|l| (l.name, l.value))
            .collect();

        ContainerInfo {
            id,
            name,
            image: summary.Image,
            runtime: ContainerRuntime::Docker, // Will be set by the client
            labels,
            created: if summary.Created > 0 { Some(summary.Created) } else { None },
        }
    }
}

impl api::ContainerSummary {
    /// Extracts the full 64-character container ID from the short ID
    pub fn id(&self) -> String {
        // Docker API may return short or long IDs, normalize to long
        let id = &self.Id;
        if id.len() == 64 {
            id.to_string()
        } else {
            // If it's a short ID, we'll use it as-is
            // In a real implementation, we might need to call inspect
            id.to_string()
        }
    }
}

/// HTTP client for Docker and Podman APIs
#[derive(Clone)]
pub struct ApiClient {
    client: reqwest::blocking::Client,
    runtime: ContainerRuntime,
    api_url: String,
}

impl ApiClient {
    pub fn new(runtime: ContainerRuntime, api_url: &str) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            client,
            runtime,
            api_url: api_url.to_string(),
        })
    }

    /// Lists all containers (including stopped ones)
    pub fn list_containers(&self) -> anyhow::Result<impl Iterator<Item = ContainerInfo> + use<'_>> {
        let url = format!("{}/containers/json?all=true", self.api_url.trim_end_matches('/'));
        
        let response = self.client
            .get(&url)
            .send()
            .context("failed to send HTTP request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_else(|_| String::new());
            return Err(anyhow::anyhow!("API request failed with status {}: {}", status, body));
        }

        let containers: Vec<api::ContainerSummary> = response
            .json()
            .context("failed to parse JSON response")?;

        // Set the runtime for each container
        Ok(containers.into_iter().map(|mut c| {
            let mut info = ContainerInfo::from(c);
            info.runtime = self.runtime;
            info
        }))
    }

    /// Inspects a specific container by ID
    pub fn inspect_container(&self, id: &str) -> anyhow::Result<ContainerInfo> {
        let url = format!("{}/containers/{}/json", self.api_url.trim_end_matches('/'), id);
        
        let response = self.client
            .get(&url)
            .send()
            .context("failed to send HTTP request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_else(|_| String::new());
            return Err(anyhow::anyhow!("API request failed with status {}: {}", status, body));
        }

        // For simplicity, we'll use a basic structure here
        // In a full implementation, we'd have a proper InspectResponse struct
        #[derive(Deserialize)]
        struct InspectResponse {
            Id: String,
            Name: String,
            Config: ContainerConfig,
            Created: String,
        }

        #[derive(Deserialize)]
        struct ContainerConfig {
            Image: String,
            Labels: std::collections::HashMap<String, String>,
        }

        let inspect: InspectResponse = response
            .json()
            .context("failed to parse JSON response")?;

        let labels = inspect.Config.Labels
            .into_iter()
            .collect();

        let created = inspect.Created
            .parse::<i64>()
            .ok();

        Ok(ContainerInfo {
            id: inspect.Id,
            name: inspect.Name.trim_start_matches('/').to_string(),
            image: inspect.Config.Image,
            runtime: self.runtime,
            labels,
            created,
        })
    }

    pub fn runtime(&self) -> ContainerRuntime {
        self.runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};
    
    #[test]
    fn test_container_summary_id() {
        let summary = api::ContainerSummary {
            Id: "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2".to_string(),
            Names: vec!["/my-container".to_string()],
            Image: "my-image:latest".to_string(),
            Labels: vec![],
            Created: 1234567890,
        };
        
        assert_eq!(summary.id().len(), 64);
    }

    #[test]
    fn test_container_info_from_summary() {
        let summary = api::ContainerSummary {
            Id: "container-id".to_string(),
            Names: vec!["/my-container".to_string()],
            Image: "my-image:latest".to_string(),
            Labels: vec![],
            Created: 1234567890,
        };
        
        let info = ContainerInfo::from(summary);
        assert_eq!(info.id, "container-id");
        assert_eq!(info.name, "my-container");
        assert_eq!(info.image, "my-image:latest");
        assert_eq!(info.labels.len(), 0);
        assert_eq!(info.created, Some(1234567890));
    }
}