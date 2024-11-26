//! # utils file for k8s module of cgroupv2 plugin
//!

use anyhow::*;
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

use super::token::Token;
use crate::cgroupv2::CgroupV2Metric;

#[derive(Debug)]
pub struct CgroupV2MetricFile {
    /// Name of the pod.
    pub name: String,
    /// Path to the cgroup cpu stat file.
    pub path_cpu: PathBuf,
    /// Path to the cgroup memory stat file.
    pub path_memory: PathBuf,
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

/// Check if a specific file is a dir.
/// Used to know if cgroup v2 are used
///
/// # Arguments
///
/// - `path` : Path of file or directory to check
///
/// # Return
///
/// - Boolean error ocurres when path of a file or directory is not available
/// - Error ocurres when an other kind of issue appears on the path
pub fn is_accessible_dir(path: &Path) -> Result<bool, std::io::Error> {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                Ok(true) // Directory available
            } else {
                Ok(false) // Not a directory
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(false) // Path not exist
            } else {
                Err(e) // Permissions errors or other
            }
        }
    }
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
    let main_hash_map: HashMap<String, (String, String, String)> =
        rt.block_on(async { get_existing_pods(hostname, kubernetes_api_url, token).await })?;

    // For each File in the root path
    for entry in entries {
        let path = entry?.path();
        let mut path_cloned_cpu = path.clone();
        let mut path_cloned_memory = path.clone();

        if path.is_dir() {
            let path_cpu = path_cloned_cpu.clone();
            let path_memory = path_cloned_memory.clone();

            let file_name = path.file_name().ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let dir_uid = file_name
                .to_str()
                .with_context(|| format!("Filename is not valid UTF-8: {:?}", path))?;

            if !(dir_uid.ends_with(".slice")) {
                continue;
            }

            let dir_uid_mod = dir_uid.strip_suffix(".slice").unwrap_or(dir_uid);

            let root_file_name = root_directory_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let truncated_prefix = root_file_name
                .to_str()
                .with_context(|| format!("filename is not valid UTF-8: {path:?}"))?;

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
            let (name, namespace, node): (String, String, String) = match main_hash_map.get(&name_to_seek.to_owned()) {
                Some((name, namespace, node)) => (name.to_owned(), namespace.to_owned(), node.to_owned()),
                None => ("".to_owned(), "".to_owned(), "".to_owned()),
            };

            let file_cpu: File = File::open(&path_cloned_cpu)
                .with_context(|| format!("failed to open file {}", path_cloned_cpu.display()))?;
            let file_memory: File = File::open(&path_cloned_memory)
                .with_context(|| format!("failed to open file {}", path_cloned_memory.display()))?;

            // Let's create the new metric and push it to the vector of metrics
            vec_file_metric.push(CgroupV2MetricFile {
                name: name.clone(),
                path_cpu,
                file_cpu,
                path_memory,
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

/// Extracts the metrics from the file.
pub fn gather_value(file: &mut CgroupV2MetricFile, content_buffer: &mut String) -> anyhow::Result<CgroupV2Metric> {
    content_buffer.clear(); // Clear before use

    file.file_cpu
        .read_to_string(content_buffer)
        .with_context(|| format!("Unable to gather cgroup v2 metrics by reading file {}", file.name))?;
    file.file_cpu.rewind()?;

    file.file_memory
        .read_to_string(content_buffer)
        .with_context(|| format!("Unable to gather cgroup v2 metrics by reading file {}", file.name))?;
    file.file_memory.rewind()?;

    let mut new_metric =
        CgroupV2Metric::from_str(content_buffer).with_context(|| format!("failed to parse {}", file.name))?;

    new_metric.name = file.name.clone();
    new_metric.namespace = file.namespace.clone();
    new_metric.uid = file.uid.clone();
    new_metric.node = file.node.clone();
    Ok(new_metric)
}

/// Returns a HashMap where the key is the uid used and the value is a tuple containing it's name, namespace and node
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
        let size = items.as_array().unwrap_or(&vec![]).len();
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
            log::debug!("Found matching pod: {} in namespace {}", pod_name, pod_namespace);
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
        let size = items.as_array().unwrap_or(&vec![]).len();
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

// ------------------ //
// --- UNIT TESTS --- //
// ------------------ //
#[cfg(test)]
mod tests {
    use super::{super::plugin::TokenRetrieval, *};
    use std::fs::File;
    use std::path::PathBuf;

    // Tests to evaluate existing files and dir
    #[test]
    fn test_is_cgroups_v2() {
        let tmp: PathBuf = std::env::temp_dir();
        let root: PathBuf = tmp.join("test-alumet-plugin-k8s/is_cgroupv2");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir: PathBuf = root.join("myDirCgroup");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(is_accessible_dir(&dir).unwrap());

        let non_existent_path: PathBuf = root.join("non_existent_dir");
        assert!(!is_accessible_dir(&non_existent_path).unwrap());

        let file_path: PathBuf = root.join("test_file.txt");
        std::fs::write(&file_path, "This is a test file.").unwrap();
        assert!(!is_accessible_dir(&file_path).unwrap());

        assert!(!is_accessible_dir(&PathBuf::new()).unwrap());
        std::fs::remove_dir_all(&root).unwrap();
    }

    // Test to simulate arborescence of kubernetes pods
    #[test]
    fn test_list_metric_file_in_dir() {
        let tmp: PathBuf = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-folder.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir: PathBuf = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(is_accessible_dir(&dir).unwrap());

        let sub_dir: [PathBuf; 4] = [
            dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/"),
            dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice/"),
            dir.join("kubepods-burstable-podccq5da1942a81912549c152a49b5f9b1.slice/"),
            dir.join("kubepods-burstable-podd87dz3z8z09de2b4b526361248c902c0.slice/"),
        ];

        for i in 0..4 {
            std::fs::create_dir_all(&sub_dir[i]).unwrap();
            assert!(is_accessible_dir(&sub_dir[i]).unwrap());
        }

        for i in 0..4 {
            std::fs::write(sub_dir[i].join("cpu.stat"), "test_cpu").unwrap();
            assert!(is_accessible_dir(&sub_dir[i]).unwrap());
            std::fs::write(sub_dir[i].join("memory.stat"), "test_memory").unwrap();
            assert!(is_accessible_dir(&sub_dir[i]).unwrap());
        }

        let list_met_file: anyhow::Result<Vec<CgroupV2MetricFile>> =
            list_metric_file_in_dir(&dir, "", "", &Token::new(TokenRetrieval::Kubectl));

        let list_pod_name: [&str; 4] = [
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
                log::error!("Error reading list_met_file: {:?}", err);
                assert!(false);
            }
        }
        assert!(true);
    }

    #[test]
    fn test_gather_value() {
        let tmp: PathBuf = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-gather.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(is_accessible_dir(&dir).unwrap());

        let sub_dir: PathBuf = dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        std::fs::create_dir_all(&sub_dir).unwrap();
        assert!(is_accessible_dir(&sub_dir).unwrap());

        let path_cpu: PathBuf = sub_dir.join("cpu.stat");
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

        let path_memory: PathBuf = sub_dir.join("memory.stat");
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

        // CPU stat file
        let file_cpu = match File::open(&path_cpu) {
            Err(why) => panic!("couldn't open {}: {}", path_cpu.display(), why),
            Ok(file_cpu) => file_cpu,
        };

        // Memory stat file
        let file_memory = match File::open(&path_memory) {
            Err(why) => panic!("couldn't open {}: {}", path_memory.display(), why),
            Ok(file_memory) => file_memory,
        };

        let cgroup_v2_metric_file = CgroupV2MetricFile {
            name: "testing_pod".to_string(),
            path_cpu,
            file_cpu,
            path_memory,
            file_memory,
            uid: "uid_test".to_string(),
            namespace: "namespace_test".to_string(),
            node: "node_test".to_owned(),
        };

        let mut cgroup: CgroupV2MetricFile = cgroup_v2_metric_file;
        let mut content = String::new();
        let result = gather_value(&mut cgroup, &mut content);

        if let Ok(CgroupV2Metric {
            name,
            uid,
            namespace,
            node,
            time_used_tot,
            time_used_user_mode,
            time_used_system_mode,
            anon_used_mem,
            file_mem,
            kernel_mem,
            pagetables_mem,
        }) = result
        {
            assert_eq!(name, "testing_pod".to_owned());
            assert_eq!(uid, "uid_test".to_owned());
            assert_eq!(namespace, "namespace_test".to_owned());
            assert_eq!(node, "node_test".to_owned());
            assert_eq!(time_used_tot, 8335557927);
            assert_eq!(time_used_user_mode, 4728882396);
            assert_eq!(time_used_system_mode, 3606675531);
            assert_eq!(anon_used_mem, 8335557927);
            assert_eq!(file_mem, 4728882396);
            assert_eq!(kernel_mem, 3686400);
            assert_eq!(pagetables_mem, 0);
        } else {
            assert!(result.is_err());
        }
    }
}
