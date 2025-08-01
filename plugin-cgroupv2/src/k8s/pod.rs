use super::token::Token;
use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::header;
use serde_json::Value;
use std::{collections::HashMap, path::Path};

const CGROUP_POD_PATTERN: &str = r"^kubepods(?:-([a-z]+))?-pod([0-9a-f_]+)\.slice$";

#[derive(Default)]
pub struct PodInfos {
    pub name: String,
    pub namespace: String,
    pub node: String,
}

/// Returns informations about running pods on a specific node
/// the HashMap have pod uid as key and PodInfos structure as value
/// Note for future improvements: this function could take a Client as parameter to avoid creating a new one everytime
pub async fn get_node_pods_infos(
    node: &str,
    kubernetes_api_url: &str,
    token: &Token,
) -> anyhow::Result<HashMap<String, PodInfos>> {
    if kubernetes_api_url.is_empty() {
        log::error!("Cannot get node pods informations because the kubernetes api url is empty");
        return Ok(HashMap::new());
    }

    let client = create_authenticated_client(token).await?;

    let mut pod_infos_by_uid = HashMap::new();
    if let Ok(data) = fetch_node_pod_data(&client, kubernetes_api_url, node).await {
        if let Some(items) = data.get("items") {
            for item in items.as_array().unwrap_or(&vec![]) {
                if let Some((pod_uid, pod_infos)) = extract_pod_infos(item) {
                    pod_infos_by_uid.insert(pod_uid, pod_infos);
                }
            }
        } else {
            log::debug!("No items part found in the JSON response.");
        }
    }
    Ok(pod_infos_by_uid)
}

/// Reads files in a filesystem to associate a cgroup of a pod uid to a kubernetes pod name
/// Note for future improvements: this function could take a Client as parameter to avoid creating a new one everytime
pub async fn get_pod_infos(uid: &str, node: &str, kubernetes_api_url: &str, token: &Token) -> anyhow::Result<PodInfos> {
    let new_uid = uid.replace('_', "-");
    if kubernetes_api_url.is_empty() {
        return Ok(PodInfos::default());
    }

    let client = create_authenticated_client(token).await?;

    if let Ok(data) = fetch_node_pod_data(&client, kubernetes_api_url, node).await {
        if let Some(items) = data.get("items") {
            for item in items.as_array().unwrap_or(&vec![]) {
                if let Some((pod_uid, pod_infos)) = extract_pod_infos(item) {
                    if pod_uid == new_uid {
                        log::debug!(
                            "Found matching pod: {} in namespace {}",
                            pod_infos.name,
                            pod_infos.namespace
                        );
                        return Ok(pod_infos);
                    }
                }
            }
        } else {
            log::debug!("No items found in the JSON response.");
        }
    }
    Ok(PodInfos::default())
}

/// Creates a HTTP client with authentication token
async fn create_authenticated_client(token: &Token) -> Result<reqwest::Client> {
    let token_value = match token.get_value().await {
        Ok(token_value) => token_value,
        Err(e) => {
            log::error!("Could not retrieve the token, got: {e}");
            return Err(anyhow!("Failed to retrieve the token, got: {e}"));
        }
    };
    let mut headers = header::HeaderMap::new();
    headers.insert(header::AUTHORIZATION, format!("Bearer {}", token_value).parse()?);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .default_headers(headers)
        .build()?;

    Ok(client)
}

/// Execute and get json data from request
async fn fetch_pod_data(client: &reqwest::Client, url: &str) -> Result<Value> {
    let response = client.get(url).send().await?;

    let data: Value = match response.json().await {
        Ok(value) => value,
        Err(err) => {
            log::error!("Error parsing JSON: {}", err);
            return Err(anyhow!("Failed to parse JSON response"));
        }
    };
    Ok(data)
}

/// Fetches pod data from Kubernetes API and retry on all nodes in case not found on the specific node
async fn fetch_node_pod_data(client: &reqwest::Client, kubernetes_api_url: &str, node: &str) -> Result<Value> {
    let all_nodes_url = format!("{}/api/v1/pods/", kubernetes_api_url);
    let (initial_request_url, is_filtered_by_node) = if node.is_empty() {
        (all_nodes_url.to_owned(), false)
    } else {
        (format!("{}?fieldSelector=spec.nodeName={}", all_nodes_url, node), true)
    };

    let mut data = fetch_pod_data(client, &initial_request_url).await?;

    let size = if let Some(items) = data.get("items") {
        items.as_array().map(|a| a.len()).unwrap_or(0)
    } else {
        log::debug!("Data is empty or not available.");
        0
    };

    // If the node was not found and the request was filtered by node, we retry for all nodes
    if size == 0 && is_filtered_by_node {
        data = fetch_pod_data(client, &all_nodes_url).await?;
    }

    Ok(data)
}

/// Extracts pod informations from JSON data
fn extract_pod_infos(item: &Value) -> Option<(String, PodInfos)> {
    let metadata = item.get("metadata").unwrap_or(&Value::Null);
    let spec = item.get("spec").unwrap_or(&Value::Null);
    let annotations = metadata.get("annotations").unwrap_or(&Value::Null);

    // Get config hash from annotations or use UID as fallback
    let mut config_hash = annotations
        .get("kubernetes.io/config.hash")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if config_hash.is_empty() {
        config_hash = metadata.get("uid").and_then(|v| v.as_str()).unwrap_or("");
        if config_hash.is_empty() {
            return None;
        }
    }

    let pod_name = metadata.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let pod_namespace = metadata.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
    let pod_node = spec.get("nodeName").and_then(|v| v.as_str()).unwrap_or("");

    Some((
        String::from(config_hash),
        PodInfos {
            name: pod_name.to_owned(),
            namespace: pod_namespace.to_owned(),
            node: pod_node.to_owned(),
        },
    ))
}

// K8S Cgroupv2 Layout
// /sys/fs/cgroup/kubepods.slice => top level
//
// /sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice => contains informations about pods that have BestEffort QoS Class
// /sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podxxxxx.slice => cgroup files (memory.stat, cpu.stat, ...) about one pod
//
// /sys/fs/cgroup/kubepods.slice/kubepods-burstable.slice => contains informations about pods that have Burstable QoS Class
// /sys/fs/cgroup/kubepods.slice/kubepods-burstable.slice/kubepods-burstable-podxxxxx.slice => cgroup files (memory.stat, cpu.stat, ...)  about one pod

// /sys/fs/cgroup/kubepods.slice/kubepods-podxxxxxxxxx.slice => cgroup files (memory.stat, cpu.stat, ...)  about one pod which that Guaranteed QoS Class
pub fn is_cgroup_pod_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    let pattern = Regex::new(CGROUP_POD_PATTERN).unwrap();

    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => pattern.is_match(name),
        None => false,
    }
}

pub fn get_uid_from_cgroup_dir(path: &Path) -> Result<String, anyhow::Error> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("Invalid path: cannot extract file name"))?;

    let pattern = Regex::new(CGROUP_POD_PATTERN).map_err(|e| anyhow!(e.to_string()))?;

    let caps = pattern
        .captures(name)
        .ok_or_else(|| anyhow!("Path does not match cgroup pattern"))?;

    caps.get(2)
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| anyhow!("Pod UID not found in the capture group"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
    const TOKEN_CONTENT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiZXhwIjo0MTAyNDQ0ODAwLCJuYW1lIjoiVDNzdDFuZyBUMGszbiJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";

    #[test]
    fn test_is_cgroup_dir() {
        // These should match the regex
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();
        let valid = vec![
            "kubepods.slice/kubepods-pod5f32d849_6210_4886_a48d_e0d90e1d0206.slice",
            "kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod4299aab7_818f_401d_8261_491b94e9afb7.slice",
            "kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod85b951fd_6954_491d_bcf4_c7490e49e399.slice",
            "kubepods.slice/kubepods-burstable.slice/kubepods-burstable-pod85d44a30_d0c7_4ed9_b0dc_e4c3c87ef724.slice",
            "kubepods.slice/kubepods-burstable.slice/kubepods-burstable-podd4afbb8c_b4e4_4f0c_bc93_c25332638532.slice",
        ];

        // These should NOT match the regex
        let invalid = vec![
            "kubepods-podabc123.slice.extra",
            "kubepods-podABC123.slice",
            "kubepods-pod123@456.slice",
            "kubepods.slice",
            "random-dir",
        ];

        for path in valid {
            let full_path = root.join(path);
            fs::create_dir_all(&full_path).unwrap();
            assert!(
                is_cgroup_pod_dir(Path::new(&full_path)),
                "Expected true for valid: {}",
                path
            );
        }

        for path in invalid {
            let full_path = root.join(path);
            fs::create_dir_all(&full_path).unwrap();
            assert!(
                !is_cgroup_pod_dir(Path::new(&full_path)),
                "Expected false for invalid: {}",
                path
            );
        }
    }

    // Test `get_node_pods_infos` with an empty kubernetes api url
    #[tokio::test]
    async fn test_get_node_pods_infos_with_empty_url() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_3");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "test_node";
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_node_pods_infos(node, "", &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }

    // Test `get_node_pods_infos` with JSON send in fake server to a specific token
    #[tokio::test]
    async fn test_get_node_pods_infos_with_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_4");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "metadata": {
                                "name": "pod1",
                                "namespace": "default",
                                "uid": "01234",
                                "annotations": {
                                    "kubernetes.io/config.hash": "hash1"
                                }
                            },
                            "spec": {
                                "nodeName": "node1"
                            }
                        },
                        {
                            "metadata": {
                                "name": "pod2",
                                "namespace": "default",
                                "uid": "56789"
                            },
                            "spec": {
                                "nodeName": "node2"
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_node_pods_infos(node, kubernetes_api_url.as_str(), &token)
            .await
            .unwrap();

        let pod_infos_hash1 = result.get("hash1").unwrap();
        let pod_infos_56789 = result.get("56789").unwrap();

        assert_eq!(pod_infos_hash1.name, "pod1");
        assert_eq!(pod_infos_hash1.namespace, "default");
        assert_eq!(pod_infos_hash1.node, "node1");
        assert_eq!(pod_infos_56789.name, "pod2");
        assert_eq!(pod_infos_56789.namespace, "default");
        assert_eq!(pod_infos_56789.node, "node2");
    }

    //// Test `get_node_pods_infos` with JSON send in fake server to a specific token,
    //// with some of them missing in the JSON
    #[tokio::test]
    async fn test_get_node_pods_infos_with_half_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_5");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "metadata": {
                                "namespace": "default",
                                "annotations": {
                                    "kubernetes.io/config.hash": "hash1"
                                }
                            },
                            "nodeName": "node1",
                        },
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_node_pods_infos(node, kubernetes_api_url.as_str(), &token)
            .await
            .unwrap();

        let pod_infos = result.get("hash1").unwrap();
        assert_eq!(pod_infos.name, "");
        assert_eq!(pod_infos.namespace, "default");
        assert_eq!(pod_infos.node, "");
    }

    // Test `get_node_pods_infos` with JSON parsing and URL error
    #[tokio::test]
    async fn test_get_node_pods_infos_with_url_and_json_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_6");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "invalid")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "invalid": {
                                "invalid": "invalid"
                            },
                        },
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_node_pods_infos(node, kubernetes_api_url.as_str(), &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    // Test `get_node_pods_infos` with JSON reading cursor error
    #[tokio::test]
    async fn test_get_node_pods_infos_with_cursor_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_7");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": []
                })
                .to_string(),
            )
            .create_async()
            .await;

        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_node_pods_infos(node, kubernetes_api_url.as_str(), &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    // Test `get_pod_infos` with not existing token file and empty kubernetes api url
    #[tokio::test]
    async fn test_get_pod_infos_with_empty_url() {
        let uid = "test_uid";
        let node = "test_node";
        let token = Token::with_default_file();

        let result = get_pod_infos(uid, node, "", &token).await;
        assert!(result.is_ok());

        let pod_infos = result.unwrap();
        assert!(pod_infos.name.is_empty());
        assert!(pod_infos.namespace.is_empty());
        assert!(pod_infos.node.is_empty());
    }

    // Test `get_pod_infos` with valid existing token file and empty kubernetes api url
    #[tokio::test]
    async fn test_get_pod_infos_with_valid_token_and_empty_url() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_8");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let uid = "test_uid";
        let node = "test_node";
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_pod_infos(uid, node, "", &token).await;
        assert!(result.is_ok());

        let pod_infos = result.unwrap();
        assert!(pod_infos.name.is_empty());
        assert!(pod_infos.namespace.is_empty());
        assert!(pod_infos.node.is_empty());
    }

    // Test `get_pod_infos` with JSON send in fake server to a specific token and get valid data
    #[tokio::test]
    async fn test_get_pod_infos_with_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_9");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "metadata": {
                                "name": "pod1",
                                "namespace": "default",
                                "uid": "01234",
                                "annotations": {
                                    "kubernetes.io/config.hash": "hash1"
                                }
                            },
                            "spec": {
                                "nodeName": "node1"
                            }
                        },
                        {
                            "metadata": {
                                "name": "pod2",
                                "namespace": "default",
                                "uid": "56789"
                            },
                            "spec": {
                                "nodeName": "node2"
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uid = "hash1";
        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let pod_infos = get_pod_infos(uid, node, kubernetes_api_url.as_str(), &token)
            .await
            .unwrap();
        assert_eq!(pod_infos.name, "pod1");
        assert_eq!(pod_infos.namespace, "default");
        assert_eq!(pod_infos.node, "node1");
    }

    // Test `get_pod_infos` with JSON send in fake server to a specific token and get data,
    // with some of them missing in the JSON
    #[tokio::test]
    async fn test_get_pod_infos_with_half_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_10");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "metadata": {
                                "namespace": "default",
                                "uid": "01234",
                                "annotations": {
                                    "kubernetes.io/config.hash": "hash1"
                                }
                            },
                            "nodeName": "node1",
                        },
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uid = "hash1";
        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let pod_infos = get_pod_infos(uid, node, kubernetes_api_url.as_str(), &token)
            .await
            .unwrap();
        assert_eq!(pod_infos.name, "");
        assert_eq!(pod_infos.namespace, "default");
        assert_eq!(pod_infos.node, "");
    }

    // Test `get_pod_infos` with JSON parsing and URL error
    #[tokio::test]
    async fn test_get_pod_infos_with_url_and_json_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_11");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "invalid")
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "invalid": {
                                "invalid": "invalid"
                            },
                        },
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uid = "hash1";
        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_pod_infos(uid, node, kubernetes_api_url.as_str(), &token).await;
        assert!(result.is_ok());

        let pod_infos = result.unwrap();
        assert!(pod_infos.name.is_empty());
        assert!(pod_infos.namespace.is_empty());
        assert!(pod_infos.node.is_empty());
    }

    // Test `get_pod_infos` with uid error
    #[tokio::test]
    async fn test_get_pod_infos_with_uid_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_12");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": [
                        {
                            "metadata": {
                                "name": "pod1",
                                "namespace": "default",
                                "uid": "01234",
                                "annotations": {
                                    "kubernetes.io/config.hash": "hash1"
                                }
                            },
                            "spec": {
                                "nodeName": "node1"
                            }
                        },
                        {
                            "metadata": {
                                "name": "pod2",
                                "namespace": "default",
                                "uid": "56789"
                            },
                            "spec": {
                                "nodeName": "node2"
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uid = "invalid";
        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_pod_infos(uid, node, kubernetes_api_url.as_str(), &token).await;
        assert!(result.is_ok());

        let pod_infos = result.unwrap();
        assert!(pod_infos.node.is_empty());
        assert!(pod_infos.namespace.is_empty());
        assert!(pod_infos.node.is_empty());
    }

    // Test `get_pod_infos` with JSON reading cursor error
    #[tokio::test]
    async fn test_get_pod_infos_with_cursor_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_13");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": []
                })
                .to_string(),
            )
            .create_async()
            .await;

        let uid = "invalid";
        let kubernetes_api_url = server.url();
        let token = Token::with_file(path.to_str().unwrap().to_owned());

        let result = get_pod_infos(uid, node, kubernetes_api_url.as_str(), &token).await;
        assert!(result.is_ok());

        let pod_infos = result.unwrap();
        assert!(pod_infos.name.is_empty());
        assert!(pod_infos.namespace.is_empty());
        assert!(pod_infos.node.is_empty());
    }
}
