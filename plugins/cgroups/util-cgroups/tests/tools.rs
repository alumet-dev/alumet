use anyhow::anyhow;
use std::collections::HashSet;
use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::tempdir;
use util_cgroups::detect::{ClosureCallbacks, Config, callback};
use util_cgroups::hierarchy::CgroupVersion;
use util_cgroups::measure::v2::mock::{CpuStatMock, MemoryStatMock, MockFileCgroupKV};
use util_cgroups::{CgroupDetector, CgroupHierarchy};

pub const WAITING: Duration = Duration::from_secs(5);

// mod cgroupv1;
// mod cgroupv2;

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

//
// Functions used to create files and folders
//

/// Creates some files needed by cgroupv2 to work. in the file structure hierarchy, files are created under [`root`].
/// By default their contents are the default implemented in their structure.
/// # Example
///
/// ```
/// use std::path::PathBuf;
/// use plugin_cgroup::hierarchy::CgroupVersion;
///
/// let dir_path = root.join(name);
/// create_files(&dir_path, cgroup_version)?;
/// ```
pub fn create_files_cgroupv2(root: &PathBuf) -> Result<(), anyhow::Error> {
    let cpu_stat_mock_file = CpuStatMock::default();
    let mem_stat_mock_file: MemoryStatMock = MemoryStatMock::default();

    let file_path_cpu = root.join("cpu.stat");
    let mut file_cpu = File::create(file_path_cpu.clone())?;
    cpu_stat_mock_file.write_to_file(&mut file_cpu)?;
    assert!(file_path_cpu.exists());

    let file_path_mem_stat = root.join("memory.stat");
    let mut file_mem_stat = File::create(file_path_mem_stat.clone())?;
    mem_stat_mock_file.write_to_file(&mut file_mem_stat)?;
    assert!(file_path_mem_stat.exists());

    Ok(())
}

pub fn create_files_cgroupv1(root: &PathBuf, hierarchy: CgroupHierarchy) -> Result<PathBuf, anyhow::Error> {
    if hierarchy.available_controllers().contains(&String::from("cpuacct")) {
        let cpu_stat_mock = CpuStatMock::default();

        let file_path_cpu = root.join("cpuacct.usage");
        let mut file_cpu = File::create(file_path_cpu.clone())?;
        cpu_stat_mock.write_to_file(&mut file_cpu)?;
        assert!(file_path_cpu.exists());

        Ok(file_path_cpu)
    } else if hierarchy.available_controllers().contains(&String::from("memory")) {
        let memory_stat_mock = MemoryStatMock::default();

        let file_path_mem_stat = root.join("memory.usage_in_bytes");
        let mut file_mem_stat = File::create(file_path_mem_stat.clone())?;
        memory_stat_mock.write_to_file(&mut file_mem_stat)?;
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
/// ```
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

    //Creation of CgroupHierarchy
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

#[test]
fn test_cgroup_detector_creation() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());
    let root = tempfile::tempdir()?;

    let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
    let config = Config::default();
    let handler = ClosureCallbacks {
        on_cgroups_created: callback(|cgroups| {
            println!("new cgroups detected: {cgroups:?}");
            Ok(())
        }),
        on_cgroups_removed: callback(|_| todo!()),
    };
    let cgroup_detector = CgroupDetector::new(hierarchy, config, handler);
    assert!(cgroup_detector.is_ok());
    Ok(())
}

#[test]
fn test_cgroup_detector_creation_bad_perms() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());
    let root = tempfile::tempdir()?;
    let file_path = root.path().join("toto");
    fs::create_dir(&file_path).expect("Failed to create temp directory");
    let bad_permissions = fs::Permissions::from_mode(0o000);
    fs::set_permissions(&root, bad_permissions).expect("Failed to set permissions");

    let hierarchy = CgroupHierarchy::manually_unchecked(file_path, CgroupVersion::V2, vec!["cpu", "memory"]);
    let config = Config::default();
    let handler = ClosureCallbacks {
        on_cgroups_created: callback(|cgroups| {
            println!("new cgroups detected: {cgroups:?}");
            Ok(())
        }),
        on_cgroups_removed: callback(|_cgroups| todo!()),
    };
    let cgroup_detector = CgroupDetector::new(hierarchy, config, handler);
    assert!(cgroup_detector.is_err());
    let correct_permissions = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&root, correct_permissions).expect("Failed to set permissions");
    Ok(())
}

#[test]
fn test_cgroup_detector_creation_broken_symbolic_link() -> anyhow::Result<()> {
    let _ = env_logger::try_init_from_env(env_logger::Env::default());
    let root = tempfile::tempdir()?;
    //Create a symbolic link ...
    let target_folder_path = root.path().join("target.txt");
    fs::create_dir(&target_folder_path).expect("Can't create target folder");
    let symlink_path = root.path().join("symbolinc_link");
    symlink(&target_folder_path, &symlink_path).expect("Can't create the symbolic link");
    // and break it
    fs::remove_dir(&target_folder_path).expect("Failed to delete target file");

    let hierarchy = CgroupHierarchy::manually_unchecked(symlink_path, CgroupVersion::V2, vec!["cpu", "memory"]);
    let config = Config::default();
    let handler = ClosureCallbacks {
        on_cgroups_created: callback(|cgroups| {
            println!("new cgroups detected: {cgroups:?}");
            Ok(())
        }),
        on_cgroups_removed: callback(|_| todo!()),
    };
    let cgroup_detector = CgroupDetector::new(hierarchy, config, handler);
    assert!(cgroup_detector.is_err());

    Ok(())
}
