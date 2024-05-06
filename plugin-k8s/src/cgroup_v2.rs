use std::{
    fs::{self, File},
    io::{self, Error, Read, Seek},
    path::{Path, PathBuf},
    result::Result::Ok,
    str::FromStr,
    vec,
};

use anyhow::*;

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
}

impl CgroupV2MetricFile {
    /// Create a new CgroupV2MetricFile structure from a name, a path and a File
    fn new(name: String, path_entry: PathBuf, file: File) -> CgroupV2MetricFile {
        CgroupV2MetricFile {
            name: name,
            path: path_entry,
            file: file,
        }
    }
}

/// Check if a specific file is a dir. Used to know if cgroupv2 are used
pub fn is_accessible_dir(path: &Path) -> bool {
    path.is_dir()
}

/// Retrieves the name of a cgroup without its prefix.
///
/// ## Example
/// ```ignore
/// use std::path::PathBuf;
/// 
/// let path = PathBuf::from("myPath/kubepods-burstable.slice/kubepods-burstable-podABCD");
/// assert_eq!(retrieve_name(&path, &"kubepods-burstable-".to_string()).unwrap(),"podABCD");
/// ```
fn retrieve_name(path: &Path, prefix: &str) -> anyhow::Result<String> {
    // Get the last component of the path (file or directory name)
    if path.as_os_str().is_empty() {
        anyhow::bail!("Path can't be empty")
    }
    if prefix != "" {
        let file_name = path.file_name().unwrap().to_string_lossy();
        let file_strip_prefix = file_name.strip_prefix(prefix).unwrap_or(&file_name);
        let file_strip_suffix = file_strip_prefix.strip_suffix(&".slice").unwrap_or(file_strip_prefix);
        return Ok(file_strip_suffix.to_owned());
    }
    Ok("".to_owned())
}

/// Returns a Vector of CgroupV2MetricFile associated to pods availables under a given directory.
fn list_metric_file_in_dir(root_directory_path: &String, prefix: &String) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let root_path = Path::new(root_directory_path);
    let prefix_path = Path::new(prefix);
    let dir = root_path.join(prefix_path);
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let mut path = entry?.path();
        if path.is_dir() {
            let _dir_name = path.file_name().expect("Impossible to write dir name");
            let truncated_prefix = prefix.strip_suffix(".slice/").unwrap_or(&prefix);
            let mut new_prefix = truncated_prefix.to_owned();
            new_prefix.push_str("-");
            let name = retrieve_name(&path, &new_prefix.to_owned())?;
            path.push("cpu.stat");
            let file = File::open(&path).with_context(|| format!("failed to open file {}", path.display()))?;
            vec_file_metric.push(CgroupV2MetricFile {
                name: name,
                path: path,
                file: file,
            });
        }
    }
    return Ok(vec_file_metric);
}

/// This function list all k8s pods availables, using 3 sub-directory to look in:
/// For each subdirectory, we look in if there is a directory/ies about pods and we add it
/// to a vector. All subdirectory are visited with the help of <list_metric_file_in_dir> function.
pub fn list_all_k8s_pods_file() -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    let root_directory_path: &str = "/sys/fs/cgroup/kubepods.slice/";
    if !Path::new(root_directory_path).exists() {
        return Ok(final_li_metric_file);
    }
    let all_sub_dir: Vec<String> = vec![
        "".to_string(),
        "kubepods-besteffort.slice/".to_string(),
        "kubepods-burstable.slice/".to_string(),
    ];
    for prefix in all_sub_dir {
        let mut result_vec = list_metric_file_in_dir(&root_directory_path.to_owned(), &prefix.to_owned())?;
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
    Ok(new_metric)
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
    fn test_retrieve_name() {
        let tmp = std::env::temp_dir();
        let root: std::path::PathBuf = tmp.join("test-alumet-plugin-k8s/kubepods-besteffort.slice");
        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }
        let burstable_dir = root.join("kubepods-burstable.slice");
        let besteffort_dir = root.join("kubepods-besteffort.slice");
        std::fs::create_dir_all(&burstable_dir).unwrap();
        std::fs::create_dir_all(&besteffort_dir).unwrap();

        let a = burstable_dir.join("kubepods-burstable-pod32a1942cb9a81912549c152a49b5f9b1.slice");
        let b = burstable_dir.join("kubepods-besteffort-podd9209de2b4b526361248c9dcf3e702c0.slice");
        let c = besteffort_dir.join("kubepods-besteffort-pod32a1942cb9a81912549c152a49b5f9b1.slice");
        let d = besteffort_dir.join("kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0.slice");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::create_dir_all(&c).unwrap();
        std::fs::create_dir_all(&d).unwrap();

        assert_eq!(
            retrieve_name(&a, &"kubepods-burstable-".to_string()).unwrap(),
            "pod32a1942cb9a81912549c152a49b5f9b1"
        );
        assert_eq!(
            retrieve_name(&b, &"kubepods-burstable-".to_string()).unwrap(),
            "kubepods-besteffort-podd9209de2b4b526361248c9dcf3e702c0"
        );
        assert_eq!(
            retrieve_name(&c, &"kubepods-besteffort-".to_string()).unwrap(),
            "pod32a1942cb9a81912549c152a49b5f9b1"
        );
        assert_eq!(
            retrieve_name(&d, &"kubepods-besteffort-".to_string()).unwrap(),
            "kubepods-burstable-podd9209de2b4b526361248c9dcf3e702c0"
        );

        let path_buf = PathBuf::from("");
        let name = "zkjbf".to_string();
        match retrieve_name(path_buf.as_path(), &name) {
            Ok(_) => assert!(false),
            Err(_) => assert!(true),
        };
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
        let li_met_file: anyhow::Result<Vec<CgroupV2MetricFile>> = list_metric_file_in_dir(
            &root.into_os_string().into_string().unwrap(),
            &"kubepods-burstable.slice/".to_owned(),
        );
        let list_pod_name = [
            "pod32a1942cb9a81912549c152a49b5f9b1",
            "podd9209de2b4b526361248c9dcf3e702c0",
            "podccq5da1942a81912549c152a49b5f9b1",
            "podd87dz3z8z09de2b4b526361248c902c0",
        ];

        match li_met_file {
            Ok(unwrap_li) => {
                assert_eq!(unwrap_li.len(), 4);
                for pod in unwrap_li {
                    if !list_pod_name.contains(&pod.name.as_str()) {
                        log::error!("Pod name not in the list: {}", pod.name);
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

        let mut my_cgroup_test_file: CgroupV2MetricFile =
            CgroupV2MetricFile::new("testing_pod".to_string(), path_file, file);
        let mut content_file = String::new();
        let res_metric = gather_value(&mut my_cgroup_test_file, &mut content_file);
        if let Ok(CgroupV2Metric {
            name,
            time_used_tot,
            time_used_user_mode,
            time_used_system_mode,
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
