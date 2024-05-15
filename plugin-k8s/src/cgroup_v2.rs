use std::{
    fs::{self, File},
    io::{Read, Seek},
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


/// Returns a Vector of CgroupV2MetricFile associated to pods availables under a given directory.
fn list_metric_file_in_dir(root_directory_path: &Path) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut vec_file_metric: Vec<CgroupV2MetricFile> = Vec::new();
    let entries = fs::read_dir(root_directory_path)?;
    for entry in entries {
        let path = entry?.path();
        let mut path_cloned = path.clone();
        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_str().with_context(|| format!("filename is not valid UTF-8: {path:?}"))?;
            let dir_name_mod = dir_name.strip_suffix(".slice").unwrap_or(&dir_name);
            let truncated_prefix = root_directory_path.file_name().unwrap().to_str().with_context(|| format!("filename is not valid UTF-8: {path:?}"))?;
            let mut new_prefix = truncated_prefix.strip_suffix(".slice").unwrap_or(&truncated_prefix).to_owned();
            new_prefix.push_str("-");            
            let name = dir_name_mod.strip_prefix(&new_prefix).unwrap_or(&dir_name_mod);
            path_cloned.push("cpu.stat");
            let file = File::open(&path_cloned).with_context(|| format!("failed to open file {}", path_cloned.display()))?;
            vec_file_metric.push(CgroupV2MetricFile {
                name: name.to_owned(),
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
pub fn list_all_k8s_pods_file(root_directory_str: &str) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut final_li_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    let root_directory_path = Path::new(root_directory_str);
    if !root_directory_path.exists() {
        return Ok(final_li_metric_file);
    }
    // Add the root for all subdirectory:
    let mut all_sub_dir: Vec<PathBuf>  = vec![root_directory_path.to_owned()];
    // Iterate in the root directory and add to the vec all folder ending with "".slice"
    // On unix, folders are files, files are files and peripherals are also files
    for file in fs::read_dir(root_directory_str)?{
        let path = file?.path();
        let file_name = path.file_name().unwrap().to_str().with_context(|| format!("filename is not valid UTF-8: {path:?}"))?;
        if path.is_dir() && file_name.ends_with(".slice"){
            all_sub_dir.push(path);            
        }
    }

    for prefix in all_sub_dir {
        let mut result_vec = list_metric_file_in_dir(&prefix.to_owned())?;
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
        let li_met_file: anyhow::Result<Vec<CgroupV2MetricFile>> = list_metric_file_in_dir(&burstable_dir);
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
