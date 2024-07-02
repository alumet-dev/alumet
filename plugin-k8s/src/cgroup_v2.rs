use anyhow::*;
use reqwest::{self, header};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::Command,
    result::Result::Ok,
    str::FromStr,
    vec,
};

use crate::parsing_cgroupv2::CgroupV2Metric;

/// CgroupV2MetricFile represents a file containing cgroup v2 data about cpu usage.
///
/// Note that the file contains multiple metrics.
#[derive(Debug)]
pub struct CgroupV2MetricFile {
    /// Name of the pod.
    pub name: String,
    /// Path to the file.
    pub path: PathBuf,
    /// Opened file descriptor.
    pub file: File,
    /// UID of the pod.
    pub uid: String,
    /// Namespace of the pod.
    pub namespace: String,
    /// Node of the pod.
    pub node: String,
}

impl CgroupV2MetricFile {
    /// Create a new CgroupV2MetricFile structure from a name, a path and a File
    fn new(
        name: String,
        path_entry: PathBuf,
        file: File,
        uid: String,
        namespace: String,
        node: String,
    ) -> CgroupV2MetricFile {
        CgroupV2MetricFile {
            name,
            path: path_entry,
            file,
            uid,
            namespace,
            node,
        }
    }
}

/// Check if a specific file is a dir. Used to know if cgroupv2 are used
pub fn is_accessible_dir(path: &Path) -> bool {
    path.is_dir()
}

/// Returns a Vector of CgroupV2MetricFile associated to pods availables under a given directory.
fn list_metric_file_in_dir(root_directory_path: &Path, hostname: String) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    let entries = fs::read_dir(root_directory_path)?;
    // Let's create a runtime to await async function and fullfill hashmap
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let main_hash_map: HashMap<String, (String, String, String)> =
        rt.block_on(async { get_existing_pods(hostname).await })?;

    // For each File in the root path
    for entry in entries {
        let path = entry?.path();
        let mut path_cloned = path.clone();

        if path.is_dir() {
            let path_mf = path_cloned.clone();

            let file_name = path.file_name().ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let dir_uid = file_name
                .to_str()
                .with_context(|| format!("Filename is not valid UTF-8: {:?}", path))?;

            let dir_uid_mod = dir_uid.strip_suffix(".slice").unwrap_or(&dir_uid);

            let root_file_name = root_directory_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let truncated_prefix = root_file_name
                .to_str()
                .with_context(|| format!("filename is not valid UTF-8: {path:?}"))?;

            let mut new_prefix = truncated_prefix
                .strip_suffix(".slice")
                .unwrap_or(&truncated_prefix)
                .to_owned();

            new_prefix.push_str("-");
            let uid = dir_uid_mod.strip_prefix(&new_prefix).unwrap_or(&dir_uid_mod);
            path_cloned.push("cpu.stat");
            let name_to_seek_raw = uid.strip_prefix("pod").unwrap_or(uid);
            let name_to_seek = name_to_seek_raw.replace("_", "-"); // Replace _ with - to match with hashmap

            // Look in the hashmap if there is a tuple (name, namespace, node) associated to the uid of the cgroup
            let (name, namespace, node): (String, String, String) = match main_hash_map.get(&name_to_seek.to_owned()) {
                Some((name, namespace, node)) => (name.to_owned(), namespace.to_owned(), node.to_owned()),
                None => ("".to_owned(), "".to_owned(), "".to_owned()),
            };

            let file: File =
                File::open(&path_cloned).with_context(|| format!("failed to open file {}", path_cloned.display()))?;
            // Let's create the new metric and push it to the vector of metrics
            vec_file_metric.push(CgroupV2MetricFile {
                name: name.clone(),
                path: path_mf,
                file: file,
                uid: uid.to_owned(),
                namespace: namespace.clone(),
                node: node.clone(),
            });
        }
    }
    return Ok(vec_file_metric);
}

/// This function list all k8s pods availables, using sub-directories to look in:
/// For each subdirectory, we look in if there is a directory/ies about pods and we add it
/// to a vector. All subdirectory are visited with the help of <list_metric_file_in_dir> function.
pub fn list_all_k8s_pods_file(root_directory_path: &Path, hostname: String) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    if !root_directory_path.exists() {
        return Ok(final_li_metric_file);
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
        let mut result_vec = list_metric_file_in_dir(&prefix.to_owned(), hostname.clone())?;
        final_li_metric_file.append(&mut result_vec);
    }
    return Ok(final_li_metric_file);
}

/// Extracts the metrics from the file.
pub fn gather_value(file: &mut CgroupV2MetricFile, content_buffer: &mut String) -> anyhow::Result<CgroupV2Metric> {
    content_buffer.clear(); //Clear before use
    file.file
        .read_to_string(content_buffer)
        .with_context(|| format!("Unable to gather cgroup v2 metrics by reading file {}", file.name))?;
    file.file.rewind()?;
    let mut new_metric =
        CgroupV2Metric::from_str(&content_buffer).with_context(|| format!("failed to parse {}", file.name))?;
    new_metric.name = file.name.clone();
    new_metric.namespace = file.namespace.clone();
    new_metric.uid = file.uid.clone();
    new_metric.node = file.node.clone();
    Ok(new_metric)
}

/// Returns a HashMap where the key is the uid used and the value is a tuple containing it's name, namespace and node
pub async fn get_existing_pods(node: String) -> anyhow::Result<HashMap<String, (String, String, String)>> {
    let Ok(output) = Command::new("kubectl")
        .args(&["create", "token", "alumet-reader"])
        .output()
    else {
        return Ok(HashMap::new());
    };

    let token = String::from_utf8_lossy(&output.stdout);
    let token = token.trim();
    let api_url_root = "https://10.22.80.14:6443/api/v1/pods/";
    let api_url = if node == "" {
        api_url_root.to_owned()
    } else {
        let tmp = format!("{}?fieldSelector=spec.nodeName={}", api_url_root, node);
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
        let size = items.as_array().unwrap_or(&vec![]).len(); // If the node was not found i.e. no item in the response, we call the API again with all nodes
        if size == 0 {
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
            if config_hash == "" {
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

    return Ok(hash_map_to_ret);
}

/// Reads files in a filesystem to associate a cgroup of a poduid to a kubernetes pod name
pub async fn get_pod_name(uid: String, node: String) -> anyhow::Result<(String, String, String)> {
    let new_uid = uid.replace("_", "-");
    let Ok(output) = Command::new("kubectl")
        .args(&["create", "token", "alumet-reader"])
        .output()
    else {
        return Ok(("".to_string(), "".to_string(), "".to_string()));
    };

    let token = String::from_utf8_lossy(&output.stdout);
    let token = token.trim();
    let api_url_root = "https://10.22.80.14:6443/api/v1/pods/";
    let api_url = if node == "" {
        api_url_root.to_owned()
    } else {
        let tmp = format!("{}?fieldSelector=spec.nodeName={}", api_url_root, node);
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
        let size = items.as_array().unwrap_or(&vec![]).len(); // If the node was not found i.e. no item in the response, we call the API again with all nodes
        if size == 0 {
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
            if config_hash == "" {
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
    use super::*;

    #[test]
    fn test_is_cgroups_v2() {
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/is_cgroupv2");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let cgroupv2_dir = root.join("myDirCgroup");
        std::fs::create_dir_all(&cgroupv2_dir).unwrap();
        assert!(is_accessible_dir(Path::new(&cgroupv2_dir)));
        assert!(!is_accessible_dir(std::path::Path::new(
            "test-alumet-plugin-k8s/is_cgroupv2/myDirCgroup_bad"
        )));
    }

    #[test]
    fn test_list_metric_file_in_dir() {
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-folder.slice/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let burstable_dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&burstable_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");
        let b = burstable_dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice/");
        let c = burstable_dir.join("kubepods-burstable-podccq5da1942a81912549c152a49b5f9b1.slice/");
        let d = burstable_dir.join("kubepods-burstable-podd87dz3z8z09de2b4b526361248c902c0.slice/");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(a.join("cpu.stat"), "en").unwrap();
        std::fs::write(b.join("cpu.stat"), "fr").unwrap();
        std::fs::write(c.join("cpu.stat"), "sv").unwrap();
        std::fs::write(d.join("cpu.stat"), "ne").unwrap();
        let li_met_file: anyhow::Result<Vec<CgroupV2MetricFile>> =
            list_metric_file_in_dir(&burstable_dir, "".to_string());
        let list_pod_name = [
            "pod32a1942cb9a81912549c152a49b5f9b1",
            "podd9209de2b4b526361248c9dcf3e702c0",
            "podccq5da1942a81912549c152a49b5f9b1",
            "podd87dz3z8z09de2b4b526361248c902c0",
        ];

        match li_met_file {
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
                log::error!("Error reading li_met_file: {:?}", err);
                assert!(false);
            }
        }
        assert!(true);
    }
    #[test]
    fn test_gather_value() {
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-gather.slice/");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let burstable_dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&burstable_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice/");

        std::fs::create_dir_all(&a).unwrap();
        let path_file = a.join("cpu.stat");
        std::fs::write(
            path_file.clone(),
            format!(
                "usage_usec 8335557927\n
                user_usec 4728882396\n
                system_usec 3606675531\n
                nr_periods 0\n
                nr_throttled 0\n
                throttled_usec 0"
            ),
        )
        .unwrap();

        let file = match File::open(&path_file) {
            Err(why) => panic!("couldn't open {}: {}", path_file.display(), why),
            Ok(file) => file,
        };

        let mut my_cgroup_test_file: CgroupV2MetricFile = CgroupV2MetricFile::new(
            "testing_pod".to_string(),
            path_file,
            file,
            "uid_test".to_string(),
            "namespace_test".to_string(),
            "node_test".to_owned(),
        );
        let mut content_file = String::new();
        let res_metric = gather_value(&mut my_cgroup_test_file, &mut content_file);
        if let Ok(CgroupV2Metric {
            name,
            time_used_tot,
            time_used_user_mode,
            time_used_system_mode,
            uid: _uid,
            namespace: _ns,
            node: _nd,
        }) = res_metric
        {
            assert_eq!(name, "testing_pod".to_owned());
            assert_eq!(time_used_tot, 8335557927);
            assert_eq!(time_used_user_mode, 4728882396);
            assert_eq!(time_used_system_mode, 3606675531);
        } else {
            assert!(false);
        }
    }
}
