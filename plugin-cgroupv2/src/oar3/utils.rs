use alumet::resources::ResourceConsumer;
use anyhow::{Context, Result};
use std::{
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
    result::Result::Ok,
    str::FromStr,
    vec,
};

use crate::cgroupv2::CgroupMeasurements;

/// CgroupV2MetricFile represents a file containing cgroup v2 data about cpu usage.
///
/// Note that the file contains multiple metrics.
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
}

impl CgroupV2MetricFile {
    /// Create a new CgroupV2MetricFile structure from a name, a path and a File.
    fn new(
        name: String,
        consumer_cpu: ResourceConsumer,
        consumer_memory: ResourceConsumer,
        file_cpu: File,
        file_memory: File,
    ) -> CgroupV2MetricFile {
        CgroupV2MetricFile {
            name,
            consumer_cpu,
            consumer_memory,
            file_cpu,
            file_memory,
        }
    }
}

/// Returns a Vector of CgroupV2MetricFile associated to pods available under a given directory.
fn list_metric_file_in_dir(root_directory_path: &Path) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut vec_file_metric = Vec::new();
    let entries = fs::read_dir(root_directory_path)?;

    // For each Entry in the directory
    for entry in entries {
        let path = entry?.path();
        let mut path_cloned_cpu = path.clone();
        let mut path_cloned_memory = path.clone();

        path_cloned_cpu.push("cpu.stat");
        path_cloned_memory.push("memory.stat");

        if (path_cloned_cpu.exists() && path_cloned_cpu.is_file())
            && (path_cloned_memory.exists() && path_cloned_memory.is_file())
        {
            let file_name = path.file_name().ok_or_else(|| anyhow::anyhow!("No file name found"))?;
            let file_cpu = File::open(&path_cloned_cpu)
                .with_context(|| format!("Failed to open file {}", path_cloned_cpu.display()))?;
            let file_memory = File::open(&path_cloned_memory)
                .with_context(|| format!("Failed to open file {}", path_cloned_memory.display()))?;

            // CPU resource consumer for cpu.stat file in cgroup
            let consumer_cpu = ResourceConsumer::ControlGroup {
                path: path_cloned_cpu
                    .to_str()
                    .expect("Path to 'cpu.stat' must be valid UTF8")
                    .to_string()
                    .into(),
            };
            // Memory resource consumer for cpu.stat file in cgroup
            let consumer_memory = ResourceConsumer::ControlGroup {
                path: path_cloned_memory
                    .to_str()
                    .expect("Path to 'memory.stat' must to be valid UTF8")
                    .to_string()
                    .into(),
            };

            // Let's create the new metric and push it to the vector of metrics
            vec_file_metric.push(CgroupV2MetricFile {
                name: file_name.to_str().context("Filename is not valid UTF-8")?.to_string(),
                consumer_cpu,
                consumer_memory,
                file_cpu,
                file_memory,
            });
        }
    }
    Ok(vec_file_metric)
}

/// This function list all cgroup files available, using sub-directories to look in:
/// For each subdirectory, we look in if there is a directory/ies about pods and we add it
/// to a vector. All subdirectory are visited with the help of <list_metric_file_in_dir> function.
pub fn list_all_file(root_directory_path: &Path) -> anyhow::Result<Vec<CgroupV2MetricFile>> {
    let mut final_list_metric_file: Vec<CgroupV2MetricFile> = Vec::new();
    if !root_directory_path.exists() {
        return Ok(final_list_metric_file);
    }
    // Add the root for all subdirectory:
    let mut all_sub_dir: Vec<PathBuf> = vec![root_directory_path.to_owned()];
    // Iterate in the root directory and add to the vec all folders
    // On unix, folders are files, files are files and peripherals are also files
    for file in fs::read_dir(root_directory_path)? {
        let path = file?.path();
        if path.is_dir() {
            all_sub_dir.push(path);
        }
    }

    for prefix in all_sub_dir {
        let mut result_vec = list_metric_file_in_dir(&prefix.to_owned())?;
        final_list_metric_file.append(&mut result_vec);
    }
    Ok(final_list_metric_file)
}

/// Extracts the metrics from data files of cgroup.
///
/// # Arguments
///
/// - `CgroupV2MetricFile` : Get structure parameters to use cgroup data.
/// - `content_buffer` : Buffer where we store content of cgroup data file.
///
/// # Return
///
/// - Error if CPU data file is not found.
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

    Ok(new_metric)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // Test `list_metric_file_in_dir` function to simulate arborescence of kubernetes pods
    #[test]
    fn test_list_metric_file_in_dir() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-oar/kubepods-folder.slice/");

        if root.exists() {
            std::fs::remove_dir_all(&root).unwrap();
        }

        let dir = root.join("kubepods-burstable.slice/");
        std::fs::create_dir_all(&dir).unwrap();

        let sub_dir = [
            dir.join("32a1942cb9a81912549c152a49b5f9b1"),
            dir.join("d9209de2b4b526361248c9dcf3e702c0"),
            dir.join("ccq5da1942a81912549c152a49b5f9b1"),
            dir.join("d87dz3z8z09de2b4b526361248c902c0"),
        ];

        for i in 0..4 {
            std::fs::create_dir_all(&sub_dir[i]).unwrap();
        }

        for i in 0..4 {
            std::fs::write(sub_dir[i].join("cpu.stat"), "test_cpu").unwrap();
            std::fs::write(sub_dir[i].join("memory.stat"), "test_memory").unwrap();
        }

        let list_met_file = list_metric_file_in_dir(&dir);
        let list_pod_name = [
            "32a1942cb9a81912549c152a49b5f9b1",
            "d9209de2b4b526361248c9dcf3e702c0",
            "ccq5da1942a81912549c152a49b5f9b1",
            "d87dz3z8z09de2b4b526361248c902c0",
        ];

        match list_met_file {
            Ok(unwrap_list) => {
                assert_eq!(unwrap_list.len(), 4);
                for pod in unwrap_list {
                    if !list_pod_name.contains(&pod.name.as_str()) {
                        log::error!("Pod name not in the list: {}", pod.name);
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
        let root = tmp.path().join("test-alumet-plugin-oar/kubepods-invalid-gather.slice/");

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
        };

        let mut content_buffer = String::new();
        let result = gather_value(&mut metric_file, &mut content_buffer);

        result.expect("gather_value get invalid data");
    }

    // Test `gather_value` function with valid values
    #[test]
    fn test_gather_value_with_valid_values() {
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("test-alumet-plugin-oar/kubepods-gather.slice/");

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

        let mut cgroup = CgroupV2MetricFile::new(
            "testing_pod".to_string(),
            consumer_cpu,
            consumer_memory,
            file_cpu,
            file_memory,
        );

        let mut content = String::new();
        let result = gather_value(&mut cgroup, &mut content);

        if let Ok(CgroupMeasurements {
            pod_name,
            cpu_time_total,
            cpu_time_user_mode,
            cpu_time_system_mode,
            memory_anonymous,
            memory_file,
            memory_kernel,
            memory_pagetables,
            pod_uid: _uid,
            namespace: _ns,
            node: _nd,
        }) = result
        {
            assert_eq!(pod_name, "testing_pod".to_owned());
            assert_eq!(cpu_time_total, 8335557927);
            assert_eq!(cpu_time_user_mode, 4728882396);
            assert_eq!(cpu_time_system_mode, 3606675531);
            assert_eq!(memory_anonymous, 8335557927);
            assert_eq!(memory_file, 4728882396);
            assert_eq!(memory_kernel, 3686400);
            assert_eq!(memory_pagetables, 0);
        }
    }

    // Test `list_all_file` function with different file system
    #[test]
    fn test_list_all_file() {
        let tmp = tempdir().unwrap();
        let root = tmp.path();

        let path = root.join("non_existent");
        let result = list_all_file(&path);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());

        let result = list_all_file(root);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());

        let sub_dir = root.join("sub_dir");
        fs::create_dir(&sub_dir).unwrap();

        let list_file_name = ["file1.txt", "file2.txt", "file3.txt", "file4.txt"];
        for i in 0..4 {
            File::create(sub_dir.join(list_file_name[i])).unwrap();
        }

        let result = list_all_file(root);
        assert!(result.is_ok());
    }
}
