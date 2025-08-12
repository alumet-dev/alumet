use std::path::Path;

use anyhow::Context;
use rustc_hash::FxHashMap;

use super::token::Token;
use api::PodList;

/// Kubernetes API client (limited capabilities, just what we need).
#[derive(Clone)]
pub struct ApiClient {
    client: reqwest::blocking::Client,
    auth_token: Token,
    k8s_api_pods_route: String,
}

/// Relevant informations about a pod.
#[derive(Debug, Default, Clone)]
pub struct PodInfos {
    pub uid: String,
    pub name: String,
    pub namespace: String,
    pub node: String,
}

/// Automatically-refreshed pod registry: keep track of the pods on a given node.
#[derive(Clone)]
pub struct AutoNodePodRegistry {
    client: ApiClient,
    node: String,
    // TODO use uuid instead of string to reduce memory consumption
    pods: FxHashMap<String, PodInfos>,
}

/// Encoding/decoding of the K8S API responses.
/// Fields that we don't need are not included, serde will skip them.
mod api {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct PodList {
        pub items: Vec<Pod>,
    }

    #[derive(Deserialize)]
    pub struct Pod {
        pub metadata: ObjectMeta,
        pub spec: PodSpec,
    }

    #[derive(Deserialize)]
    pub struct ObjectMeta {
        pub name: String,
        pub namespace: String,
        pub uid: String,
    }

    #[derive(Deserialize)]
    pub struct PodSpec {
        #[serde(rename = "nodeName")]
        pub node_name: String,
    }
}

impl From<api::Pod> for PodInfos {
    fn from(pod: api::Pod) -> Self {
        PodInfos {
            uid: pod.metadata.uid,
            name: pod.metadata.name,
            namespace: pod.metadata.namespace,
            node: pod.spec.node_name,
        }
    }
}

impl ApiClient {
    pub fn new(k8s_api_url: &str, auth_token: Token) -> anyhow::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .context("failed to build http client")?;

        let k8s_api_pods_route = format!("{k8s_api_url}/api/v1/pods");

        Ok(Self {
            auth_token,
            client,
            k8s_api_pods_route,
        })
    }

    pub fn list_pods(&self, node: Option<&str>) -> anyhow::Result<impl Iterator<Item = PodInfos>> {
        // get the auth token, refreshed if needed
        let token = self.auth_token.get_value().context("failed to get auth token")?;

        // prepare the request
        let mut req = self.client.get(&self.k8s_api_pods_route).bearer_auth(token);
        if let Some(node) = node {
            req = req.query(&[("fieldSelector", format!("spec.nodeName={}", node))]);
        }

        // send and parse response
        let response = req.send().context("failed to send http request")?;
        println!("response: {response:?}");
        let pods: PodList = response.json().context("failed to parse json response")?;

        // turn the response into the format we want
        let pods = pods.items.into_iter().map(PodInfos::from);
        Ok(pods)
    }
}

impl AutoNodePodRegistry {
    pub fn new(node: String, k8s_api_client: ApiClient) -> Self {
        Self {
            client: k8s_api_client,
            node,
            pods: Default::default(),
        }
    }

    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let all_pods = self
            .client
            .list_pods(Some(&self.node))
            .with_context(|| format!("failed to list K8S pods on node {}", self.node))?;
        self.pods = all_pods
            .filter(|p| p.node == self.node)
            .map(|p| (p.uid.clone(), p))
            .collect();
        Ok(())
    }

    pub fn get(&mut self, pod_uid: &str) -> anyhow::Result<Option<PodInfos>> {
        if let Some(infos) = self.pods.get(pod_uid) {
            return Ok(Some(infos.to_owned()));
        }

        // We have no info about this pod, ask the K8S API.
        self.refresh()?;

        // Is the pod here? If not, it must have been deleted in the meantime => return None.
        match self.pods.get(pod_uid) {
            Some(infos) => Ok(Some(infos.to_owned())),
            None => Ok(None),
        }
    }
}

/// Extracts the uid from a cgroup path (in the sysfs).
///
/// # Expected format
///
/// The supported formats are:
/// - `…/kubepods-burstable-pod247ca13b_bc17_4709_ab2c_4d98b5ad4fb2.slice` (matches https://pkg.go.dev/k8s.io/kubernetes/pkg/kubelet/cm#CgroupName.ToSystemd)
/// - `…/pod247ca13b-bc17-4709-ab2c-4d98b5ad4fb2.slice` (just in case)
///
/// # Containers are excluded
///
/// Returns `None` for cgroups that correspond to individual containers in a pod.
/// That is, `…/kubepods-burstable-pod{pod_uid}.slice/crio-{container_id}.scope` returns `None`,
/// while `…/kubepods-burstable-pod{pod_uid}.slice` returns `Some(pod_uid)`.
pub fn extract_pod_uid_from_cgroup(cgroup_fs_path: &Path) -> Option<String> {
    let cgroup_name = cgroup_fs_path.file_name()?.to_str()?;
    let (_prefix, suffix) = cgroup_name.rsplit_once("pod")?;
    if suffix.starts_with('s') {
        // we actually have "pods" and not "pod"
        return None;
    }
    if suffix.len() < 32 {
        // this cannot be a UUID
        return None;
    }
    let uid = suffix.strip_suffix(".slice")?.replace('_', "-");
    Some(uid.to_owned())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    use mockito::{Server, ServerGuard};
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_extract_pod_uid() {
        let pod_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod5f32d849_6210_4886_a48d_e0d90e1d0206.slice");
        assert_eq!(
            extract_pod_uid_from_cgroup(pod_path.as_path()),
            Some(String::from("5f32d849-6210-4886-a48d-e0d90e1d0206"))
        );

        let pod_parent_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice");
        assert_eq!(extract_pod_uid_from_cgroup(pod_parent_path.as_path()), None);

        let container_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod5f32d849_6210_4886_a48d_e0d90e1d0206.slice/crio-85b951fd_6954_491d_bcf4_c7490e49e399.scope");
        assert_eq!(extract_pod_uid_from_cgroup(container_path.as_path()), None);

        let pod_bad_uid_path = PathBuf::from(
            "/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-podBAD!BAD.slice",
        );
        assert_eq!(extract_pod_uid_from_cgroup(pod_bad_uid_path.as_path()), None);

        let non_k8s_app_path =
            PathBuf::from(".../user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.Terminal.slice");
        assert_eq!(extract_pod_uid_from_cgroup(non_k8s_app_path.as_path()), None);
    }

    const TOKEN_CONTENT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiZXhwIjo0MTAyNDQ0ODAwLCJuYW1lIjoiVDNzdDFuZyBUMGszbiJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";

    fn get_node_pod_infos(
        server: &ServerGuard,
        auth_token: Token,
        node: &str,
    ) -> anyhow::Result<FxHashMap<String, PodInfos>> {
        let k8s_api_url = server.url();
        let client = ApiClient::new(&k8s_api_url, auth_token)?;
        let result: FxHashMap<_, _> = client.list_pods(Some(node))?.map(|p| (p.uid.clone(), p)).collect();
        Ok(result)
    }

    // Test `get_node_pods_infos` with JSON send in fake server to a specific token
    #[test]
    fn test_get_node_pods_infos_with_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_4");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "node1";
        let url = format!("/api/v1/pods?fieldSelector=spec.nodeName%3D{}", node);
        let mut server = Server::new();
        let mock = server
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
                                "uid": "5f32d849-6210-4886-a48d-e0d90e1d0206",
                                "annotations": {
                                    "kubernetes.io/config.hash": "5f32d849-6210-4886-a48d-e0d90e1d0206"
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
                                "uid": "5fffd849-6210-4886-aaaa-e0d90e1d0206"
                            },
                            "spec": {
                                "nodeName": "node2"
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .expect(1)
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        let result = get_node_pod_infos(&server, auth_token.clone(), node).unwrap();

        let pod_infos_5f32 = result.get("5f32d849-6210-4886-a48d-e0d90e1d0206").unwrap();
        let pod_infos_5fff = result.get("5fffd849-6210-4886-aaaa-e0d90e1d0206").unwrap();
        assert_eq!(pod_infos_5f32.name, "pod1");
        assert_eq!(pod_infos_5f32.namespace, "default");
        assert_eq!(pod_infos_5f32.node, "node1");
        assert_eq!(pod_infos_5fff.name, "pod2");
        assert_eq!(pod_infos_5fff.namespace, "default");
        assert_eq!(pod_infos_5fff.node, "node2");
        mock.assert();
    }

    #[test]
    fn test_registry_with_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_4");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "node1";
        let url = format!("/api/v1/pods?fieldSelector=spec.nodeName%3D{}", node);
        let mut server = Server::new();
        let mock = server
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
                                "uid": "5f32d849-6210-4886-a48d-e0d90e1d0206",
                                "annotations": {
                                    "kubernetes.io/config.hash": "5f32d849-6210-4886-a48d-e0d90e1d0206"
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
                                "uid": "5fffd849-6210-4886-aaaa-e0d90e1d0206"
                            },
                            "spec": {
                                "nodeName": "node1"
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .expect(1)
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        let k8s_api_url = server.url();
        let k8s_api_client = ApiClient::new(&k8s_api_url, auth_token).unwrap();
        let mut registry = AutoNodePodRegistry::new(node.to_owned(), k8s_api_client);
        assert!(registry.pods.is_empty());

        // This is the only request we've got
        registry.refresh().expect("refresh should work");
        mock.assert();

        println!("refreshed: {:?}", registry.pods);

        // These should NOT generate more requests, because we've already got the pod infos
        let pod_infos_5f32 = registry.get("5f32d849-6210-4886-a48d-e0d90e1d0206").unwrap().unwrap();
        let pod_infos_5fff = registry.get("5fffd849-6210-4886-aaaa-e0d90e1d0206").unwrap().unwrap();
        assert_eq!(pod_infos_5f32.name, "pod1");
        assert_eq!(pod_infos_5f32.namespace, "default");
        assert_eq!(pod_infos_5f32.node, "node1");
        assert_eq!(pod_infos_5fff.name, "pod2");
        assert_eq!(pod_infos_5fff.namespace, "default");
        assert_eq!(pod_infos_5fff.node, "node1");
        mock.assert();
    }

    #[test]
    fn test_registry_with_missing_pod() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_4");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "node1";
        let url = format!("/api/v1/pods?fieldSelector=spec.nodeName%3D{}", node);
        let mut server = Server::new();
        let mock = server
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
                                "uid": "5f32d849-6210-4886-a48d-e0d90e1d0206",
                                "annotations": {
                                    "kubernetes.io/config.hash": "5f32d849-6210-4886-a48d-e0d90e1d0206"
                                }
                            },
                            "spec": {
                                "nodeName": "node1"
                            }
                        },
                    ]
                })
                .to_string(),
            )
            .expect(2)
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        let k8s_api_url = server.url();
        let k8s_api_client = ApiClient::new(&k8s_api_url, auth_token).unwrap();
        let mut registry = AutoNodePodRegistry::new(node.to_owned(), k8s_api_client);
        assert!(registry.pods.is_empty());

        println!("refreshed: {:?}", registry.pods);

        // These should generate TWO requests
        let pod_infos_5f32 = registry
            .get("5f32d849-6210-4886-a48d-e0d90e1d0206")
            .unwrap()
            .expect("pod 5f32 should exist"); // ok, one request
        let pod_infos_5fff = registry.get("5fffd849-6210-4886-aaaa-e0d90e1d0206").unwrap(); // missing, another request (it checks the API again, in case the pod is new)
        assert_eq!(pod_infos_5f32.name, "pod1");
        assert_eq!(pod_infos_5f32.namespace, "default");
        assert_eq!(pod_infos_5f32.node, "node1");
        assert!(pod_infos_5fff.is_none());
        mock.assert();
    }

    //// Test `get_node_pods_infos` with JSON send in fake server to a specific token,
    //// with some of them missing in the JSON
    #[test]
    fn test_get_node_pods_infos_with_half_valid_data() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_5");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods?fieldSelector=spec.nodeName%3D{}", node);
        let mut server = Server::new();
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
                                "uid": "hash1",
                            },
                            "nodeName": "node1",
                            // missing nodeSpec
                        },
                    ]
                })
                .to_string(),
            )
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        get_node_pod_infos(&server, auth_token, node).expect_err("should fail");
    }

    // Test `get_node_pods_infos` with JSON parsing and URL error
    #[test]
    fn test_get_node_pods_infos_with_url_and_json_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_6");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let mut server = Server::new();
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
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        get_node_pod_infos(&server, auth_token, node).expect_err("should fail");
    }

    // Test `get_node_pods_infos` with JSON reading cursor error
    #[test]
    fn test_get_node_pods_infos_with_cursor_error() {
        let tempdir = tempdir().unwrap();
        let root = tempdir.path();

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_7");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods?fieldSelector=spec.nodeName%3D{}", node);
        let mut server = Server::new();
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
            .create();

        let auth_token = Token::with_file(path.to_str().unwrap().to_owned());
        let result = get_node_pod_infos(&server, auth_token, node).unwrap();
        assert!(result.is_empty());
    }
}
