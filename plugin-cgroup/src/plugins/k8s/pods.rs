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
    let uid = suffix.strip_suffix(".slice")?.replace('_', "-");
    Some(uid.to_owned())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn test_extract_pod_uid() {
        let pod_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod5f32d849_6210_4886_a48d_e0d90e1d0206.slice");
        assert_eq!(
            extract_pod_uid_from_cgroup(pod_path.as_path()),
            Some(String::from("5f32d849-6210-4886-a48d-e0d90e1d0206"))
        );

        let container_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/kubepods-besteffort-pod5f32d849_6210_4886_a48d_e0d90e1d0206.slice/crio-85b951fd_6954_491d_bcf4_c7490e49e399.scope");
        assert_eq!(extract_pod_uid_from_cgroup(container_path.as_path()), None);

        let non_k8s_app_path =
            PathBuf::from(".../user.slice/user-1000.slice/user@1000.service/app.slice/app-org.gnome.Terminal.slice");
        assert_eq!(extract_pod_uid_from_cgroup(non_k8s_app_path.as_path()), None);
    }
}
