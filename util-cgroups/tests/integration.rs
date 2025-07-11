use anyhow::anyhow;
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU8;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use toml;
use util_cgroups::detect::{ClosureCallbacks, Config, callback};
use util_cgroups::hierarchy::CgroupVersion;
use util_cgroups::{CgroupDetector, CgroupHierarchy};

/// Check if a specific file is a dir. Used to know if cgroup v2 are used.
///
/// # Return value
///
/// Returns `Ok(true)` if it can be verified that `path` is a directory, and `Ok(false)` if it can be verified that it is not a directory.
/// Returns an error if the path metadata cannot be obtained.
pub fn is_accessible_dir(path: &Path) -> Result<bool, std::io::Error> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

pub trait MockFileCgroupKV: Serialize {
    fn write_to_file(&self, mut file: File) -> io::Result<()> {
        let toml_str = toml::to_string(self).expect("TOML serialization failed");

        for line in toml_str.lines() {
            if let Some((key, value)) = line.split_once(" = ") {
                writeln!(file, "{} {}", key.trim(), value.trim_matches('"'))?;
            }
        }

        Ok(())
    }

    fn replace_to_file(&self, mut file: File) -> io::Result<()> {
        // Read the current contents of the file so that it can be modified
        let mut file_content = String::new();
        file.read_to_string(&mut file_content)?;

        let mut parsed_toml: toml::Value =
            toml::de::from_str(&file_content).unwrap_or_else(|_| toml::Value::Table(Default::default()));
        let toml_str = toml::to_string(self).expect("TOML serialization failed");
        let new_toml: toml::Value = toml::de::from_str(&toml_str).expect("TOML deserialization failed");

        // Merge old data with new data
        match new_toml {
            toml::Value::Table(new_table) => {
                // If array, merge it
                let table = parsed_toml.as_table_mut().unwrap();
                for (key, value) in new_table {
                    table.insert(key, value);
                }
            }
            _ => {}
        }

        // Rewrite the updated content to the file using a space between key and value
        let mut updated_toml = String::new();
        if let Some(table) = parsed_toml.as_table() {
            for (key, value) in table {
                let value_str = value.to_string();
                updated_toml.push_str(&format!("{} {}\n", key, value_str.trim_matches('"')));
            }
        }

        // Reset file and write
        file.set_len(0)?;
        file.write_all(updated_toml.as_bytes())?;

        Ok(())
    }
}

// \\\\\\\\\\\\\\\\\\\\\\\\\
// Cgroupv2 based structures
// \\\\\\\\\\\\\\\\\\\\\\\\\

#[derive(Serialize, Debug, Default)]
pub struct CpuStatMock {
    pub usage_usec: u64,
    pub user_usec: u64,
    pub system_usec: u64,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_usec: u64,
    pub nr_bursts: u64,
    pub burst_usec: u64,
}

impl MockFileCgroupKV for CpuStatMock {}

#[derive(Serialize, Debug, Default)]
pub struct MemoryStatMock {
    pub anon: u64,
    pub file: u64,
    pub kernel: u64,
    pub kernel_stack: u64,
    pub pagetables: u64,
    pub sec_pagetables: u64,
    pub percpu: u64,
    pub sock: u64,
    pub vmalloc: u64,
    pub shmem: u64,
    pub zswap: u64,
    pub zswapped: u64,
    pub file_mapped: u64,
    pub file_dirty: u64,
    pub file_writeback: u64,
    pub swapcached: u64,
    pub anon_thp: u64,
    pub file_thp: u64,
    pub shmem_thp: u64,
    pub inactive_anon: u64,
    pub active_anon: u64,
    pub inactive_file: u64,
    pub active_file: u64,
    pub unevictable: u64,
    pub slab_reclaimable: u64,
    pub slab_unreclaimable: u64,
    pub slab: u64,
    pub workingset_refault_anon: u64,
    pub workingset_refault_file: u64,
    pub workingset_activate_anon: u64,
    pub workingset_activate_file: u64,
    pub workingset_restore_anon: u64,
    pub workingset_restore_file: u64,
    pub workingset_nodereclaim: u64,
    pub pswpin: u64,
    pub pswpout: u64,
    pub pgscan: u64,
    pub pgsteal: u64,
    pub pgscan_kswapd: u64,
    pub pgscan_direct: u64,
    pub pgscan_khugepaged: u64,
    pub pgscan_proactive: u64,
    pub pgsteal_kswapd: u64,
    pub pgsteal_direct: u64,
    pub pgsteal_khugepaged: u64,
    pub pgsteal_proactive: u64,
    pub pgfault: u64,
    pub pgmajfault: u64,
    pub pgrefill: u64,
    pub pgactivate: u64,
    pub pgdeactivate: u64,
    pub pglazyfree: u64,
    pub pglazyfreed: u64,
    pub swpin_zero: u64,
    pub swpout_zero: u64,
    pub zswpin: u64,
    pub zswpout: u64,
    pub zswpwb: u64,
    pub thp_fault_alloc: u64,
    pub thp_collapse_alloc: u64,
    pub thp_swpout: u64,
    pub thp_swpout_fallback: u64,
    pub numa_pages_migrated: u64,
    pub numa_pte_updates: u64,
    pub numa_hint_faults: u64,
    pub pgdemote_kswapd: u64,
    pub pgdemote_direct: u64,
    pub pgdemote_khugepaged: u64,
    pub pgdemote_proactive: u64,
    pub hugetlb: u64,
}

impl MockFileCgroupKV for MemoryStatMock {}

#[derive(Serialize, Debug, Default)]
pub struct MemoryCurrentMock(pub u64);
impl MemoryCurrentMock {
    pub fn write_to_file(&self, mut file: File) -> io::Result<()> {
        writeln!(file, "{}", self.0)
    }
}

// \\\\\\\\\\\\\\\\\\\\\\\\\
// Cgroupv1 based structures
// \\\\\\\\\\\\\\\\\\\\\\\\\

#[derive(Serialize, Debug, Default)]
pub struct CpuacctUsageMock {
    pub usage: u64,
}
impl MockFileCgroupKV for CpuacctUsageMock {
    fn write_to_file(&self, mut file: File) -> io::Result<()> {
        writeln!(file, "{}", self.usage)
    }
}

#[derive(Serialize, Debug, Default)]
pub struct MemoryUsageInBytes {
    pub usage: u64,
}
impl MockFileCgroupKV for MemoryUsageInBytes {
    fn write_to_file(&self, mut file: File) -> io::Result<()> {
        writeln!(file, "{}", self.usage)
    }
}

//
// Functions used to create files and folders
//

/// Creates some files needed by cgroupv2 to work. in the file structure hierarchy, files are created under [`root`].
/// By default their contents are the default implemented in their structure.
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use plugin_cgroup::hierarchy::CgroupVersion;
///
/// let dir_path = root.join(name);
/// create_files(&dir_path, cgroup_version)?;
/// ```
pub fn create_files_cgroupv2(root: &PathBuf) -> Result<(), anyhow::Error> {
    let cpu_stat_mock_file = CpuStatMock::default();
    let mem_stat_mock_file: MemoryStatMock = MemoryStatMock::default();
    let mem_current_mock_file: MemoryCurrentMock = MemoryCurrentMock(0);

    let file_path_cpu = root.join("cpu.stat");
    let file_cpu = File::create(file_path_cpu.clone())?;
    cpu_stat_mock_file.write_to_file(file_cpu)?;
    assert!(file_path_cpu.exists());

    let file_path_mem_stat = root.join("memory.stat");
    let file_mem_stat = File::create(file_path_mem_stat.clone())?;
    mem_stat_mock_file.write_to_file(file_mem_stat)?;
    assert!(file_path_mem_stat.exists());

    let file_path_mem_current = root.join("memory.current");
    let file_mem_current = File::create(file_path_mem_current.clone())?;
    mem_current_mock_file.write_to_file(file_mem_current)?;
    assert!(file_path_mem_current.exists());

    Ok(())
}

pub fn create_files_cgroupv1(root: &PathBuf, hierarchy: CgroupHierarchy) -> Result<PathBuf, anyhow::Error> {
    if hierarchy.available_controllers().contains(&String::from("cpuacct")) {
        let cpuacct_usage_mock_file = CpuacctUsageMock::default();

        let file_path_cpu = root.join("cpuacct.usage");
        let file_cpu = File::create(file_path_cpu.clone())?;
        cpuacct_usage_mock_file.write_to_file(file_cpu)?;
        assert!(file_path_cpu.exists());

        Ok(file_path_cpu)
    } else if hierarchy.available_controllers().contains(&String::from("memory")) {
        let mem_usage_in_bytes_mock_file = MemoryUsageInBytes::default();

        let file_path_mem_stat = root.join("memory.usage_in_bytes");
        let file_mem_stat = File::create(file_path_mem_stat.clone())?;
        mem_usage_in_bytes_mock_file.write_to_file(file_mem_stat)?;
        assert!(file_path_mem_stat.exists());

        Ok(file_path_mem_stat)
    } else {
        Err(anyhow!(
            "Error, controler not expected found: {:?}",
            hierarchy.available_controllers()
        ))
    }
}

/// Creates a directory within a given root path, initializes required files based on the
/// specified cgroup version, and returns the path to the created directory.
///
/// # Example
///
/// ```rust
/// use std::path::PathBuf;
/// use plugin_cgroup::hierarchy::CgroupVersion;
///
/// let root = PathBuf::from("/tmp/test_cgroup");
/// let folder = create_folder(&root, "my_cgroup", CgroupVersion::V2).unwrap();
/// assert!(folder.exists());
/// ```
pub fn create_folder(root: &PathBuf, name: &str, hierarchy: CgroupHierarchy) -> Result<PathBuf, anyhow::Error> {
    match hierarchy.version() {
        CgroupVersion::V1 => {
            // 1- Create the controller
            // 2- Create the target
            let dir_path = root.join(name);
            fs::create_dir_all(&dir_path).unwrap();
            assert!(is_accessible_dir(&dir_path).unwrap());
            if hierarchy.available_controllers().contains(&String::from("cpuacct")) {
                let _file_path = create_files_cgroupv1(&dir_path, hierarchy)?;
            } else if hierarchy.available_controllers().contains(&String::from("memory")) {
                let _file_path = create_files_cgroupv1(&dir_path, hierarchy)?;
            } else {
                return Err(anyhow!(
                    "Error, controler not expected found: {:?}",
                    hierarchy.available_controllers()
                ));
            }
            Ok(dir_path)
        }
        CgroupVersion::V2 => {
            // 1- Create the target
            // 2- Create the controller
            let dir_path = root.join(name);
            fs::create_dir_all(&dir_path).unwrap();
            assert!(is_accessible_dir(&dir_path).unwrap());
            create_files_cgroupv2(&dir_path)?;
            Ok(dir_path)
        }
    }
}

#[test]
fn no_cgroup() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;

    //Creation of CgrouHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec![""]);

    // Detection of Cgroup
    let config = Config::default();
    let f1 = callback(|cgroups| {
        for cgroup in cgroups {
            let s1: HashSet<_> = vec!["".to_string()].iter().cloned().collect();
            let s2: HashSet<_> = cgroup.hierarchy().available_controllers().iter().cloned().collect();
            let diff: Vec<_> = s1.difference(&s2).collect();
            assert!(diff.len() == 0)
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let _detector = CgroupDetector::new(hierarchy_cpu, config, handler);
}

/// \\\\\\\\\\\\\\\\\
/// Test for CgroupV1
/// \\\\\\\\\\\\\\\\\

#[test]
fn one_cgroup_created_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    let path_mem = root.join("memory");
    //Creation of CgrouHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);
    // Creation of cgroup
    assert!(create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&root, "memory", hierarchy_mem.clone()).is_ok());

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out1 = std::sync::Arc::clone(&cpt);
    let ac_out2 = std::sync::Arc::clone(&cpt);

    // Detection of Cgroup
    let config1 = Config::default();
    let config2 = Config::default();
    let f1 = callback(move |cgroups| {
        assert_eq!(cgroups.len(), 1);
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu, config1, handler);
    assert!(detector.is_ok());

    let f3 = callback(move |cgroups| {
        assert_eq!(cgroups.len(), 1);
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "memory"));
            ac_out2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_mem, config2, handler2);
    assert!(detector.is_ok());

    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn one_cgroup_created_after_v1() {
    env_logger::init_from_env(env_logger::Env::default());
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgrouHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let res_cpu = create_folder(&root, "cpuacct", hierarchy_cpu.clone()).unwrap();
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), config, handler);
    assert!(detector.is_ok());
    // Creation of cgroup
    let _res_cpu = create_folder(&res_cpu, "toto", hierarchy_cpu.clone());
    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn severale_cgroup_created_after_v1() {
    // TODO
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgrouHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    assert!(create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), config, handler);
    assert!(detector.is_ok());
    // Creation of cgroup
    let _inside_path_cpu = path_cpu.clone();
    let cpu_root = path_cpu.clone().join("Christopher_Eccleston");
    assert!(create_folder(&path_cpu, "Christopher_Eccleston", hierarchy_cpu.clone()).is_ok());

    assert!(create_folder(&cpu_root, "Rose_Tyler", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&cpu_root, "Jack_Harkness", hierarchy_cpu.clone()).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(4, cpt.load(std::sync::atomic::Ordering::SeqCst));
}
#[test]
fn cgroup_different_location_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let root1_unwraped = tempdir().unwrap();
    let root1 = root1_unwraped.path().to_path_buf();

    let version = CgroupVersion::V1;

    let path_cpu = root.join("cpuacct");
    let path_mem = root1.join("memory");

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);

    assert!(create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&root1, "memory", hierarchy_mem.clone()).is_ok());

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out1 = std::sync::Arc::clone(&cpt);
    let ac_out2 = std::sync::Arc::clone(&cpt);

    let config1 = Config::default();
    let config2 = Config::default();

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu, config1, handler);
    assert!(detector.is_ok());

    let f3 = callback(move |cgroups| {
        assert_eq!(cgroups.len(), 1);
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "memory"));
            ac_out2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let _detector = CgroupDetector::new(hierarchy_mem, config2, handler2);

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn cgroups_same_name_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;

    let path_cpu = root.join("cpuacct");
    let path_mem = root.join("memory");

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);

    let res_cpu = create_folder(&root, "cpuacct", hierarchy_cpu.clone()).expect("Can't create a folder");
    let res_mem = create_folder(&root, "memory", hierarchy_mem.clone()).expect("Can't create a folder");
    assert!(create_folder(&res_cpu, "dalek", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&res_mem, "dalek", hierarchy_mem.clone()).is_ok());
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out1 = std::sync::Arc::clone(&cpt);
    let ac_out2 = std::sync::Arc::clone(&cpt);

    // Detection of Cgroup
    let config1 = Config::default();
    let config2 = Config::default();
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu, config1, handler);
    assert!(detector.is_ok());

    let f3 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "memory"));
            ac_out2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_mem, config2, handler2);
    assert!(detector.is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(4, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn cgroup_missing_element_v1() {
    // TODO
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgrouHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    assert!(create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), config, handler);
    assert!(detector.is_ok());
    let dir_path = path_cpu.join("cpuacct");
    fs::create_dir_all(&dir_path).unwrap();
    assert!(is_accessible_dir(&dir_path).unwrap());
    let _file_path = create_files_cgroupv1(&dir_path, hierarchy_cpu).unwrap();

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn remove_cgroup_on_exec_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;

    let path_mem = root.join("memory");

    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);
    let res_mem = create_folder(&root, "memory", hierarchy_mem.clone()).expect("Can't create a folder");
    let res_mem = create_folder(&res_mem, "dalek", hierarchy_mem.clone()).unwrap();
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let config = Config::default();

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "memory"));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_mem, config, handler);
    assert!(detector.is_ok());
    assert!(fs::remove_dir_all(&res_mem).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn creation_of_cgroup_before_and_after_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    let cpu_root = path_cpu.clone().join("Peter_Capaldi");
    assert!(create_folder(&path_cpu, "Peter_Capaldi", hierarchy_cpu.clone()).is_ok());
    let _path_clara = cpu_root.clone().join("Clara_Oswald");
    assert!(create_folder(&cpu_root, "Clara_Oswald", hierarchy_cpu.clone()).is_ok());

    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), config, handler);
    assert!(detector.is_ok());

    assert!(create_folder(&cpu_root, "River_Song", hierarchy_cpu.clone()).is_ok());
    thread::sleep(Duration::from_secs(5));
    assert_eq!(4, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

/// \\\\\\\\\\\\\\\\\
/// Test for CgroupV2
/// \\\\\\\\\\\\\\\\\

#[test]
fn one_cgroup_created_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    create_files_cgroupv2(&root).unwrap();
    assert!(create_folder(&root, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(cgroup.fs_path().join("cpu.stat").exists());
            assert!(cgroup.fs_path().join("memory.stat").exists());
            assert!(cgroup.fs_path().join("memory.current").exists());
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu, Config::default(), handler);
    assert!(detector.is_ok());
    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn one_cgroup_created_after_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), Config::default(), handler);
    assert!(detector.is_ok());
    assert!(create_folder(&root, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}
#[test]
fn severale_cgroup_created_after_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua", "William_Parry", "Kirjava", "Pantalaimon"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), Config::default(), handler);
    assert!(detector.is_ok());
    let sub_root_p = create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let sub_root_k = create_folder(&root, "Kirjava", hierarchy_cpu.clone()).unwrap();

    assert!(create_folder(&sub_root_p, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&sub_root_k, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(5, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn cgroups_same_name_v2() {
    //TODO
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Pantalaimon", "Kirjava", "Lyra_Belacqua"];
            println!("------------- Cgroup: {:?}", cgroup);
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), Config::default(), handler);
    assert!(detector.is_ok());
    // Creation of cgroup
    let sub_root_p = create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let sub_root_k = create_folder(&root, "Kirjava", hierarchy_cpu.clone()).unwrap();

    assert!(create_folder(&sub_root_p, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    assert!(create_folder(&sub_root_k, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(5, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn cgroup_missing_element_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let cpu_stat_mock_file = CpuStatMock::default();
    let file_path_cpu = root.join("cpu.stat");
    let file_cpu = File::create(file_path_cpu.clone()).unwrap();
    cpu_stat_mock_file.write_to_file(file_cpu).unwrap();
    assert!(file_path_cpu.exists());

    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu"]);
    let dir_path = root.join("William_Parry");
    fs::create_dir_all(&dir_path).unwrap();
    assert!(is_accessible_dir(&dir_path).unwrap());

    let cpu_stat_mock_file = CpuStatMock::default();
    let file_path_cpu = dir_path.join("cpu.stat");
    let file_cpu = File::create(file_path_cpu.clone()).unwrap();
    cpu_stat_mock_file.write_to_file(file_cpu).unwrap();
    assert!(file_path_cpu.exists());

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.as_path()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(cgroup.fs_path().join("cpu.stat").exists());
            assert!(!cgroup.fs_path().join("memory.stat").exists());
            assert!(!cgroup.fs_path().join("memory.current").exists());
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu, Config::default(), handler);
    assert!(detector.is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn remove_cgroup_on_exec_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let sub_root_p = create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Pantalaimon"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), Config::default(), handler);
    assert!(detector.is_ok());
    assert!(fs::remove_dir_all(sub_root_p).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(2, cpt.load(std::sync::atomic::Ordering::SeqCst));
}

#[test]
fn creation_of_cgroup_before_and_after_v2() {
    let version = CgroupVersion::V2;
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    assert!(create_files_cgroupv2(&root).is_ok());

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    assert!(create_folder(&root, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());

    let cpt: std::sync::Arc<AtomicU8> = std::sync::Arc::new(AtomicU8::new(0));
    let ac_out = std::sync::Arc::clone(&cpt);
    let root_clo = std::sync::Arc::new(root.clone());
    let shared_root = std::sync::Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua", "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    });
    let f2 = callback(|_a| Ok(()));
    let handler = ClosureCallbacks {
        on_cgroups_created: f1,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_cpu.clone(), Config::default(), handler);
    assert!(detector.is_ok());
    assert!(create_folder(&root, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(Duration::from_secs(5));
    assert_eq!(3, cpt.load(std::sync::atomic::Ordering::SeqCst));
}