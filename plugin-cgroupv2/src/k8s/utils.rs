use super::token::Token;
use alumet::resources::ResourceConsumer;
use anyhow::{Context, Result};
use reqwest::{self, header};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
    result::Result::Ok,
    str::FromStr,
    vec,
};

use crate::cgroupv2::CgroupMeasurements;

#[derive(Debug)]
pub struct CgroupV2MetricFile {
    /// Name of the pod.
    pub name: String,
    /// Path to the cgroup cpu stat file.
    pub consumer_cpu: ResourceConsumer,
    /// Path to the cgroup memory stat file.
    pub consumer_memory: ResourceConsumer,
    /// Opened file descriptor for cgroup cpu stat.
    pub file_cpu: File,
    /// Opened file descriptor for cgroup memory stat.
    pub file_memory: File,
    /// UID of the pod.
    pub uid: String,
    /// Namespace of the pod.
    pub namespace: String,
    /// Node of the pod.
    pub node: String,
}

/// Returns a Vector of CgroupV2MetricFile associated to pods available under a given directory.
fn list_metric_file_in_dir(
    root_directory_path: &Path,
    hostname: &str,
    kubernetes_api_url: &str,
    token: &Token,
) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    let entries = fs::read_dir(root_directory_path)?;
    // Let's create a runtime to await async function and fill hashmap
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let main_hash_map = rt.block_on(async { get_existing_pods(hostname, kubernetes_api_url, token).await })?;

    // For each File in the root path
    for entry in entries {
        let path = entry?.path();
        let mut path_cloned_cpu = path.clone();
        let mut path_cloned_memory = path.clone();

        if path.is_dir() {
            let file_name = path.file_name().ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let dir_uid = file_name.to_str().context("Filename is not valid UTF-8")?;

            if !(dir_uid.ends_with(".slice")) {
                continue;
            }

            let dir_uid_mod = dir_uid.strip_suffix(".slice").unwrap_or(dir_uid);

            let root_file_name = root_directory_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let truncated_prefix = root_file_name.to_str().context("Filename is not valid UTF-8")?;

            let mut new_prefix = truncated_prefix
                .strip_suffix(".slice")
                .unwrap_or(truncated_prefix)
                .to_owned();

            new_prefix.push('-');
            let uid = dir_uid_mod.strip_prefix(&new_prefix).unwrap_or(dir_uid_mod);

            path_cloned_cpu.push("cpu.stat");
            path_cloned_memory.push("memory.stat");

            let name_to_seek_raw = uid.strip_prefix("pod").unwrap_or(uid);
            let name_to_seek = name_to_seek_raw.replace('_', "-"); // Replace _ with - to match with hashmap

            // Look in the hashmap if there is a tuple (name, namespace, node) associated to the uid of the cgroup
            let (name, namespace, node) = match main_hash_map.get(&name_to_seek.to_owned()) {
                Some((name, namespace, node)) => (name.to_owned(), namespace.to_owned(), node.to_owned()),
                None => ("".to_owned(), "".to_owned(), "".to_owned()),
            };

            let file_cpu = File::open(&path_cloned_cpu)
                .with_context(|| format!("failed to open file {}", path_cloned_cpu.display()))?;
            let file_memory = File::open(&path_cloned_memory)
                .with_context(|| format!("failed to open file {}", path_cloned_memory.display()))?;

            // CPU resource consumer for cpu.stat file in cgroup
            let consumer_cpu = ResourceConsumer::ControlGroup {
                path: path_cloned_cpu
                    .to_str()
                    .expect("Path to 'cpu.stat' must be valid UTF8")
                    .to_string()
                    .into(),
            };
            // Memory resource consumer for memory.stat file in cgroup
            let consumer_memory = ResourceConsumer::ControlGroup {
                path: path_cloned_memory
                    .to_str()
                    .expect("Path to 'memory.stat' must to be valid UTF8")
                    .to_string()
                    .into(),
            };

            // Let's create the new metric and push it to the vector of metrics
            vec_file_metric.push(CgroupV2MetricFile {
                name: name.clone(),
                consumer_cpu,
                file_cpu,
                consumer_memory,
                file_memory,
                uid: uid.to_owned(),
                namespace: namespace.clone(),
                node: node.clone(),
            });
        }
    }
    Ok(vec_file_metric)
}

/// This function list all k8s pods available, using sub-directories to look in
/// All subdirectory are visited with the help of <list_metric_file_in_dir> function.
pub fn list_all_k8s_pods_file(
    root_directory_path: &Path,
    hostname: String,
    kubernetes_api_url: String,
    token: &Token,
) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut final_list_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    if !root_directory_path.exists() {
        return Ok(final_list_metric_file);
    }

    // Add the root for all subdirectory:
    let mut all_sub_dir: Vec<PathBuf> = vec![root_directory_path.to_owned()];

    // Iterate in the root directory and add to the vec all folder ending with "".slice"
    // On unix, folders are files, files are files and peripherals are also files
    for file in fs::read_dir(root_directory_path)? {
        let path = file?.path();
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("No file name found"))?
            .to_str()
            .with_context(|| format!("Filename is not valid UTF-8: {:?}", path))?;
        if path.is_dir() && file_name.ends_with(".slice") {
            all_sub_dir.push(path);
        }
    }

    for prefix in all_sub_dir {
        let mut result_vec = list_metric_file_in_dir(
            &prefix.to_owned(),
            hostname.clone().as_str(),
            kubernetes_api_url.clone().as_str(),
            token,
        )?;
        final_list_metric_file.append(&mut result_vec);
    }
    Ok(final_list_metric_file)
}

/// Extracts the metrics from data files of cgroup.
///
/// # Arguments
///
/// - Get `CgroupV2MetricFile` structure parameters to use cgroup data.
/// - `content_buffer` : Buffer where we store content of cgroup data file.
///
/// # Return
///
/// - Error if CPU or memory data file are not found.
pub fn gather_value(file: &mut CgroupV2MetricFile, content_buffer: &mut String) -> anyhow::Result<CgroupMeasurements> {
    content_buffer.clear();

    // CPU cgroup data
    file.file_cpu
        .read_to_string(content_buffer)
        .context("Unable to gather cgroup v2 CPU metrics by reading file")?;
    if content_buffer.is_empty() {
        return Err(anyhow::anyhow!("CPU stat file is empty for {}", file.name));
    }
    file.file_cpu.rewind()?;

    // Memory cgroup data
    file.file_memory
        .read_to_string(content_buffer)
        .context("Unable to gather cgroup v2 memory metrics by reading file")?;
    if content_buffer.is_empty() {
        return Err(anyhow::anyhow!("Memory stat file is empty for {}", file.name));
    }
    file.file_memory.rewind()?;

    let mut new_metric =
        CgroupMeasurements::from_str(content_buffer).with_context(|| format!("failed to parse {}", file.name))?;

    new_metric.pod_name = file.name.clone();
    new_metric.namespace = file.namespace.clone();
    new_metric.pod_uid = file.uid.clone();
    new_metric.node = file.node.clone();

    Ok(new_metric)
}

/// # Returns
///
/// `HashMap` where the key is the uid used,
/// and the value is a tuple containing it's name, namespace and node
pub async fn get_existing_pods(
    node: &str,
    kubernetes_api_url: &str,
    token: &Token,
) -> anyhow::Result<HashMap<String, (String, String, String)>> {
    let token = match token.get_value().await {
        Ok(token) => token,
        Err(e) => {
            log::error!("Could not retrieve the token, got:{e}");
            return Ok(HashMap::new());
        }
    };

    if kubernetes_api_url.is_empty() {
        return Ok(HashMap::new());
    }

    let mut api_url_root = kubernetes_api_url.to_string();
    api_url_root.push_str("/api/v1/pods/");
    let mut selector = false;

    let api_url = if node.is_empty() {
        api_url_root.to_owned()
    } else {
        let tmp = format!("{}?fieldSelector=spec.nodeName={}", api_url_root, node);
        selector = true;
        tmp
    };

    let mut headers = header::HeaderMap::new();
    headers.insert(header::AUTHORIZATION, format!("Bearer {}", token).parse()?);

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .default_headers(headers)
        .build()?;

    let Ok(response) = client.get(api_url).send().await else {
        return Ok(HashMap::new());
    };

    let mut data: Value = match response.json().await {
        Ok(value) => value,
        Err(err) => {
            log::error!("Error parsing JSON: {}", err);
            Value::Null
        }
    };

    let mut hash_map_to_ret = HashMap::new();

    // let's check if the items' part contain pods to look at
    if let Some(items) = data.get("items") {
        // If the node was not found i.e. no item in the response, we call the API again with all nodes
        let size = items.as_array().map(|a| a.len()).unwrap_or(0);
        if size == 0 && selector {
            // Ask again the api, with all nodes
            let Ok(response) = client.get(api_url_root).send().await else {
                return Ok(HashMap::new());
            };

            data = match response.json().await {
                Ok(value) => value,
                Err(err) => {
                    log::error!("Error parsing JSON: {}", err);
                    Value::Null
                }
            }
        } else {
            log::debug!("Data is empty or not available.");
        }
    }

    if let Some(items) = data.get("items") {
        for item in items.as_array().unwrap_or(&vec![]) {
            let metadata = item.get("metadata").unwrap_or(&Value::Null);
            let spec = item.get("spec").unwrap_or(&Value::Null);
            let annotations = metadata.get("annotations").unwrap_or(&Value::Null);
            let mut config_hash = annotations
                .get("kubernetes.io/config.hash")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if config_hash.is_empty() {
                match metadata {
                    Value::Null => {
                        continue;
                    }
                    _ => {
                        config_hash = metadata.get("uid").and_then(|v| v.as_str()).unwrap_or("");
                    }
                }
            }

            let pod_name = metadata.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let pod_namespace = metadata.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
            let pod_node = spec.get("nodeName").and_then(|v| v.as_str()).unwrap_or("");

            hash_map_to_ret.entry(String::from(config_hash)).or_insert((
                pod_name.to_owned(),
                pod_namespace.to_owned(),
                pod_node.to_owned(),
            ));
        }
    } else {
        log::debug!("No items part found in the JSON response.");
    }

    Ok(hash_map_to_ret)
}

/// Reads files in a filesystem to associate a cgroup of a pod uid to a kubernetes pod name
pub async fn get_pod_name(
    uid: &str,
    node: &str,
    kubernetes_api_url: &str,
    token: &Token,
) -> anyhow::Result<(String, String, String)> {
    let new_uid = uid.replace('_', "-");
    let token = match token.get_value().await {
        Ok(token) => token,
        Err(e) => {
            log::error!("Could not retrieve the token, got: {e}");
            return Ok(("".to_string(), "".to_string(), "".to_string()));
        }
    };

    if kubernetes_api_url.is_empty() {
        return Ok(("".to_string(), "".to_string(), "".to_string()));
    }

    let mut api_url_root = kubernetes_api_url.to_string();
    api_url_root.push_str("/api/v1/pods/");
    let mut selector = false;

    let api_url = if node.is_empty() {
        api_url_root.to_owned()
    } else {
        let tmp = format!("{}?fieldSelector=spec.nodeName={}", api_url_root, node);
        selector = true;
        tmp
    };

    let mut headers = header::HeaderMap::new();
    headers.insert(header::AUTHORIZATION, format!("Bearer {}", token).parse()?);
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .default_headers(headers)
        .build()?;

    let Ok(response) = client.get(api_url).send().await else {
        return Ok(("".to_string(), "".to_string(), "".to_string()));
    };

    let mut data: Value = match response.json().await {
        Ok(value) => value,
        Err(err) => {
            log::error!("Error parsing JSON: {}", err);
            Value::Null
        }
    };

    // let's check if the items' part contain pods to look at
    if let Some(items) = data.get("items") {
        // If the node was not found i.e. no item in the response, we call the API again with all nodes
        let size = items.as_array().map(|a| a.len()).unwrap_or(0);
        if size == 0 && selector {
            // Ask again the api, with all nodes
            let Ok(response) = client.get(api_url_root).send().await else {
                return Ok(("".to_string(), "".to_string(), "".to_string()));
            };
            data = match response.json().await {
                Ok(value) => value,
                Err(err) => {
                    log::error!("Error parsing JSON: {}", err);
                    Value::Null
                }
            }
        } else {
            log::debug!("Data is empty or not available.");
        }
    }

    // Iterate over each item
    if let Some(items) = data.get("items") {
        for item in items.as_array().unwrap_or(&vec![]) {
            let metadata = item.get("metadata").unwrap_or(&Value::Null);
            let spec = item.get("spec").unwrap_or(&Value::Null);
            let annotations = metadata.get("annotations").unwrap_or(&Value::Null);
            let mut config_hash = annotations
                .get("kubernetes.io/config.hash")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if config_hash.is_empty() {
                match metadata {
                    Value::Null => {
                        continue;
                    }
                    _ => {
                        config_hash = metadata.get("uid").and_then(|v| v.as_str()).unwrap_or("");
                    }
                }
            }

            if config_hash == new_uid {
                let pod_name = metadata.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let pod_namespace = metadata.get("namespace").and_then(|v| v.as_str()).unwrap_or("");
                let pod_node = spec.get("nodeName").and_then(|v| v.as_str()).unwrap_or("");
                log::debug!("Found matching pod: {} in namespace {}", pod_name, pod_namespace);
                return Ok((pod_name.to_owned(), pod_namespace.to_owned(), pod_node.to_owned()));
            }
        }
    } else {
        log::debug!("No items found in the JSON response.");
    }
    Ok(("".to_string(), "".to_string(), "".to_string()))
}

#[cfg(test)]
mod tests {
    use super::{super::plugin::TokenRetrieval, *};
    use mockito::mock;
    use serde_json::json;
    use std::{fs::File, path::PathBuf};
    use tempfile::tempdir;

    // HEADER = { "alg": "HS256", "typ": "JWT" }
    // PAYLOAD = { "sub": "1234567890", "exp": 4102444800, "name": "T3st1ng T0k3n" }
    // SIGNATURE = { HMACSHA256(base64UrlEncode(header) + "." +  base64UrlEncode(payload), signature) }
    const TOKEN_CONTENT: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwiZXhwIjo0MTAyNDQ0ODAwLCJuYW1lIjoiVDNzdDFuZyBUMGszbiJ9.3vho4u0hx9QobMNbpDPvorWhTHsK9nSg2pZAGKxeVxA";

    // Test `list_metric_file_in_dir` function to simulate arborescence of kubernetes pods
    // and missing files in Kubernetes directory
    #[test]
    fn test_list_metric_file_in_dir() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/kubepods-folder.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();

        let sub_dir = dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("cpu.stat"), "test_cpu").unwrap();

        let result = list_metric_file_in_dir(&dir, "", "", &Token::new(TokenRetrieval::Kubectl));
        assert!(result.is_err());

        let sub_dir = [
            dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/"),
            dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice/"),
            dir.join("kubepods-burstable-podccq5da1942a81912549c152a49b5f9b1.slice/"),
            dir.join("kubepods-burstable-podd87dz3z8z09de2b4b526361248c902c0.slice/"),
        ];

        for i in 0..4 {
            std::fs::create_dir_all(&sub_dir[i]).unwrap();
        }

        for i in 0..4 {
            std::fs::write(sub_dir[i].join("cpu.stat"), "test_cpu").unwrap();
            std::fs::write(sub_dir[i].join("memory.stat"), "test_memory").unwrap();
        }

        let list_met_file = list_metric_file_in_dir(&dir, "", "", &Token::new(TokenRetrieval::Kubectl));
        let list_pod_name = [
            "pod32a1942cb9a81912549c152a49b5f9b1",
            "podd9209de2b4b526361248c9dcf3e702c0",
            "podccq5da1942a81912549c152a49b5f9b1",
            "podd87dz3z8z09de2b4b526361248c902c0",
        ];

        match list_met_file {
            Ok(unwrap_list) => {
                assert_eq!(unwrap_list.len(), 4);
                for pod in unwrap_list {
                    if !list_pod_name.contains(&pod.uid.as_str()) {
                        log::error!("Pod name not in the list: {}", pod.uid);
                        assert!(false);
                    }
                }
            }
            Err(err) => {
                log::error!("Reading list_met_file: {:?}", err);
                assert!(false);
            }
        }

        assert!(true);
    }

    // Test `gather_value` function with invalid data
    #[test]
    fn test_gather_value_with_invalid_data() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/kubepods-invalid-gather.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();

        let sub_dir = dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let path_cpu = sub_dir.join("cpu.stat");
        let path_memory = sub_dir.join("memory.stat");

        std::fs::write(&path_cpu, "invalid_cpu_data").unwrap();
        std::fs::write(&path_memory, "invalid_memory_data").unwrap();

        let file_cpu = File::open(&path_cpu).unwrap();
        let file_memory = File::open(&path_memory).unwrap();

        // CPU resource consumer for cpu.stat file in cgroup
        let consumer_cpu = ResourceConsumer::ControlGroup {
            path: path_cpu
                .to_str()
                .expect("Path to 'cpu.stat' must be valid UTF8")
                .to_string()
                .into(),
        };
        // Memory resource consumer for memory.stat file in cgroup
        let consumer_memory = ResourceConsumer::ControlGroup {
            path: path_memory
                .to_str()
                .expect("Path to 'memory.stat' must to be valid UTF8")
                .to_string()
                .into(),
        };

        let mut metric_file = CgroupV2MetricFile {
            name: "test-pod".to_string(),
            consumer_cpu,
            consumer_memory,
            file_cpu,
            file_memory,
            uid: "test-uid".to_string(),
            namespace: "default".to_string(),
            node: "test-node".to_string(),
        };

        let mut content_buffer = String::new();
        let result = gather_value(&mut metric_file, &mut content_buffer);

        result.expect("gather_value get invalid data");
    }

    // Test `gather_value` function with valid values
    #[test]
    fn test_gather_value_with_valid_values() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/kubepods-gather.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();

        let sub_dir = dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let path_cpu = sub_dir.join("cpu.stat");
        let path_memory = sub_dir.join("memory.stat");

        std::fs::write(
            path_cpu.clone(),
            format!(
                "
                usage_usec 8335557927\n
                user_usec 4728882396\n
                system_usec 3606675531\n
                nr_periods 0\n
                nr_throttled 0\n
                throttled_usec 0"
            ),
        )
        .unwrap();

        std::fs::write(
            path_memory.clone(),
            format!(
                "
                anon 8335557927
                file 4728882396
                kernel_stack 3686400
                pagetables 0
                percpu 16317568
                sock 12288
                shmem 233824256
                file_mapped 0
                file_dirty 20480,
                ...."
            ),
        )
        .unwrap();

        let file_cpu = match File::open(&path_cpu) {
            Err(why) => panic!("ERROR : Couldn't open {}: {}", path_cpu.display(), why),
            Ok(file_cpu) => file_cpu,
        };

        let file_memory = match File::open(&path_memory) {
            Err(why) => panic!("ERROR : Couldn't open {}: {}", path_memory.display(), why),
            Ok(file_memory) => file_memory,
        };

        // CPU resource consumer for cpu.stat file in cgroup
        let consumer_cpu = ResourceConsumer::ControlGroup {
            path: path_cpu
                .to_str()
                .expect("Path to 'cpu.stat' must be valid UTF8")
                .to_string()
                .into(),
        };
        // Memory resource consumer for memory.stat file in cgroup
        let consumer_memory = ResourceConsumer::ControlGroup {
            path: path_memory
                .to_str()
                .expect("Path to 'memory.stat' must to be valid UTF8")
                .to_string()
                .into(),
        };

        let metric_file = CgroupV2MetricFile {
            name: "testing_pod".to_string(),
            consumer_cpu,
            file_cpu,
            consumer_memory,
            file_memory,
            uid: "uid_test".to_string(),
            namespace: "namespace_test".to_string(),
            node: "node_test".to_owned(),
        };

        let mut cgroup = metric_file;
        let mut content = String::new();
        let result = gather_value(&mut cgroup, &mut content);

        if let Ok(CgroupMeasurements {
            pod_name,
            pod_uid,
            namespace,
            node,
            cpu_time_total,
            cpu_time_user_mode,
            cpu_time_system_mode,
            memory_anonymous,
            memory_file,
            memory_kernel,
            memory_pagetables,
        }) = result
        {
            assert_eq!(pod_name, "testing_pod".to_owned());
            assert_eq!(pod_uid, "uid_test".to_owned());
            assert_eq!(namespace, "namespace_test".to_owned());
            assert_eq!(node, "node_test".to_owned());
            assert_eq!(cpu_time_total, 8335557927);
            assert_eq!(cpu_time_user_mode, 4728882396);
            assert_eq!(cpu_time_system_mode, 3606675531);
            assert_eq!(memory_anonymous, 8335557927);
            assert_eq!(memory_file, 4728882396);
            assert_eq!(memory_kernel, 3686400);
            assert_eq!(memory_pagetables, 0);
        }
    }

    // Test `list_all_k8s_pods` function with a not existing file root directory
    #[tokio::test]
    async fn test_list_all_k8s_pods_file_with_no_root_directory() {
        let root = PathBuf::from("invalid");
        let hostname = "test-host".to_string();
        let kubernetes_api_url = "https://127.0.0.1:8080".to_string();
        let token = Token::new(TokenRetrieval::Kubectl);

        let result = list_all_k8s_pods_file(&root, hostname, kubernetes_api_url, &token);
        assert!(result.unwrap().is_empty());
    }

    // Test `list_all_k8s_pods` function with a no slice folders file
    #[test]
    fn test_list_all_k8s_pods_file_with_no_slice_folder() -> Result<()> {
        let temp = tempdir()?;
        let hostname = "test-host".to_string();
        let kubernetes_api_url = "https://127.0.0.1:8080".to_string();
        let token = Token::new(TokenRetrieval::Kubectl);

        fs::create_dir(temp.path().join("folder1"))?;
        fs::create_dir(temp.path().join("folder2"))?;

        let result = list_all_k8s_pods_file(temp.path(), hostname, kubernetes_api_url, &token)?;
        assert!(result.is_empty());
        Ok(())
    }

    // Test `list_all_k8s_pods` function with a file with invalid subfolder
    #[test]
    fn test_list_all_k8s_pods_file_with_invalid_subfolder() -> Result<()> {
        let temp_dir = tempdir()?;
        let hostname = "test-host".to_string();
        let kubernetes_api_url = "https://127.0.0.1:8080".to_string();
        let token = Token::new(TokenRetrieval::Kubectl);

        let slice_dir = temp_dir.path().join("folder1.slice");
        fs::create_dir(&slice_dir)?;

        File::create(slice_dir.join("invalid_file"))?;
        let result = list_all_k8s_pods_file(temp_dir.path(), hostname, kubernetes_api_url, &token);

        assert!(result.is_err());
        Ok(())
    }

    // Test `get_existing_pods` with an empty kubernetes api url
    #[tokio::test]
    async fn test_get_existing_pods_with_empty_url() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_3/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_3");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "test_node";
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_existing_pods(node, "", &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());

        std::fs::remove_dir_all(&root).unwrap();
    }

    // Test `get_existing_pods` with JSON send in fake server to a specific token
    #[tokio::test]
    async fn test_get_existing_pods_with_valid_data() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_4/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_4");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
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
            .create();

        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_existing_pods(node, kubernetes_api_url, &token).await.unwrap();

        assert_eq!(
            result.get("hash1").unwrap(),
            &("pod1".to_string(), "default".to_string(), "node1".to_string())
        );
        assert_eq!(
            result.get("56789").unwrap(),
            &("pod2".to_string(), "default".to_string(), "node2".to_string())
        );
    }

    // Test `get_existing_pods` with JSON send in fake server to a specific token,
    // with some of them missing in the JSON
    #[tokio::test]
    async fn test_get_existing_pods_with_half_valid_data() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_5/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_5");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
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
            .create();

        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_existing_pods(node, kubernetes_api_url, &token).await.unwrap();

        assert_eq!(
            result.get("hash1").unwrap(),
            &("".to_string(), "default".to_string(), "".to_string())
        );
    }

    // Test `get_existing_pods` with JSON parsing and URL error
    #[tokio::test]
    async fn test_get_existing_pods_with_url_and_json_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_6/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_6");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let _mock = mock("GET", "invalid")
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

        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_existing_pods(node, kubernetes_api_url, &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    // Test `get_existing_pods` with JSON reading cursor error
    #[tokio::test]
    async fn test_get_existing_pods_with_cursor_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_7/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_7");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": []
                })
                .to_string(),
            )
            .create();

        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_existing_pods(node, kubernetes_api_url, &token).await;
        assert!(result.is_ok());

        let map = result.unwrap();
        assert!(map.is_empty());
    }

    // Test `get_pod_name` with not existing token file and empty kubernetes api url
    #[tokio::test]
    async fn test_get_pod_name_with_empty_url() {
        let uid = "test_uid";
        let node = "test_node";
        let token = Token::new(TokenRetrieval::File);

        let result = get_pod_name(uid, node, "", &token).await;
        assert!(result.is_ok());

        let (name, namespace, node) = result.unwrap();
        assert!(name.is_empty());
        assert!(namespace.is_empty());
        assert!(node.is_empty());
    }

    // Test `get_pod_name` with valid existing token file and empty kubernetes api url
    #[tokio::test]
    async fn test_get_pod_name_with_valid_token_and_empty_url() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_8/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_8");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let uid = "test_uid";
        let node = "test_node";
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, "", &token).await;
        assert!(result.is_ok());

        let (name, namespace, node) = result.unwrap();
        assert!(name.is_empty());
        assert!(namespace.is_empty());
        assert!(node.is_empty());
    }

    // Test `get_pod_name` with JSON send in fake server to a specific token and get valid data
    #[tokio::test]
    async fn test_get_pod_name_with_valid_data() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_9/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_9");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
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
            .create();

        let uid = "hash1";
        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, kubernetes_api_url, &token).await.unwrap();
        assert_eq!(result.0, "pod1");
        assert_eq!(result.1, "default");
        assert_eq!(result.2, "node1");
    }

    // Test `get_pod_name` with JSON send in fake server to a specific token and get data,
    // with some of them missing in the JSON
    #[tokio::test]
    async fn test_get_pod_name_with_half_valid_data() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_10/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_10");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
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
            .create();

        let uid = "hash1";
        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, kubernetes_api_url, &token).await.unwrap();
        assert_eq!(result.0, "");
        assert_eq!(result.1, "default");
        assert_eq!(result.2, "");
    }

    // Test `get_pod_name` with JSON parsing and URL error
    #[tokio::test]
    async fn test_get_pod_name_with_url_and_json_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_11/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_11");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let _mock = mock("GET", "invalid")
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

        let uid = "hash1";
        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, kubernetes_api_url, &token).await;
        assert!(result.is_ok());

        let (name, namespace, node) = result.unwrap();
        assert!(name.is_empty());
        assert!(namespace.is_empty());
        assert!(node.is_empty());
    }

    // Test `get_pod_name` with uid error
    #[tokio::test]
    async fn test_get_pod_name_with_uid_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_12/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_12");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
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
            .create();

        let uid = "invalid";
        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, kubernetes_api_url, &token).await;
        assert!(result.is_ok());

        let (name, namespace, node) = result.unwrap();
        assert!(name.is_empty());
        assert!(namespace.is_empty());
        assert!(node.is_empty());
    }

    // Test `get_pod_name` with JSON reading cursor error
    #[tokio::test]
    async fn test_get_pod_name_with_cursor_error() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-k8s/var_13/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("run/secrets/kubernetes.io/serviceaccount/");
        std::fs::create_dir_all(&dir).unwrap();

        let path = dir.join("token_13");
        std::fs::write(&path, TOKEN_CONTENT).unwrap();

        let node = "pod1";
        let url = format!("/api/v1/pods/?fieldSelector=spec.nodeName={}", node);
        let _mock = mock("GET", url.as_str())
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(
                json!({
                    "items": []
                })
                .to_string(),
            )
            .create();

        let uid = "invalid";
        let kubernetes_api_url = &mockito::server_url();
        let mut token = Token::new(TokenRetrieval::File);

        token.path = Some(path.to_str().unwrap().to_owned());

        let result = get_pod_name(uid, node, kubernetes_api_url, &token).await;
        assert!(result.is_ok());

        let (name, namespace, node) = result.unwrap();
        assert!(name.is_empty());
        assert!(namespace.is_empty());
        assert!(node.is_empty());
    }
}
