use std::fs::{self, File};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use tempfile::tempdir;
use util_cgroups::detect::{ClosureCallbacks, Config, callback};
use util_cgroups::hierarchy::CgroupVersion;
use util_cgroups::measure::v2::mock::{CpuStatMock, MockFileCgroupKV};
use util_cgroups::{CgroupDetector, CgroupHierarchy};

mod tools;

#[test]
fn one_cgroup_created_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    tools::create_files_cgroupv2(&root).unwrap();
    assert!(tools::create_folder(&root, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
            assert!(cgroup.fs_path().join("cpu.stat").exists());
            assert!(cgroup.fs_path().join("memory.stat").exists());
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
    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn one_cgroup_created_after_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    tools::create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    assert!(tools::create_folder(&root, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}
#[test]
fn severale_cgroup_created_after_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    tools::create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua", "William_Parry", "Kirjava", "Pantalaimon"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    let sub_root_p = tools::create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let sub_root_k = tools::create_folder(&root, "Kirjava", hierarchy_cpu.clone()).unwrap();

    assert!(tools::create_folder(&sub_root_p, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&sub_root_k, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(5, cpt.load(Ordering::SeqCst));
}

#[test]
fn cgroups_same_name_v2() {
    //TODO
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    tools::create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Pantalaimon", "Kirjava", "Lyra_Belacqua"];
            println!("------------- Cgroup: {:?}", cgroup);
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    let sub_root_p = tools::create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let sub_root_k = tools::create_folder(&root, "Kirjava", hierarchy_cpu.clone()).unwrap();

    assert!(tools::create_folder(&sub_root_p, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&sub_root_k, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(5, cpt.load(Ordering::SeqCst));
}

#[test]
fn cgroup_missing_element_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let cpu_stat_mock_file = CpuStatMock::default();
    let file_path_cpu = root.join("cpu.stat");
    let mut file_cpu = File::create(file_path_cpu.clone()).unwrap();
    cpu_stat_mock_file.write_to_file(&mut file_cpu).unwrap();
    assert!(file_path_cpu.exists());

    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu"]);
    let dir_path = root.join("William_Parry");
    fs::create_dir_all(&dir_path).unwrap();
    assert!(tools::is_accessible_dir(&dir_path).unwrap());

    let cpu_stat_mock_file = CpuStatMock::default();
    let file_path_cpu = dir_path.join("cpu.stat");
    let mut file_cpu = File::create(file_path_cpu.clone()).unwrap();
    cpu_stat_mock_file.write_to_file(&mut file_cpu).unwrap();
    assert!(file_path_cpu.exists());

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.as_path()));
            ac_out.fetch_add(1, Ordering::SeqCst);
            assert!(cgroup.fs_path().join("cpu.stat").exists());
            assert!(!cgroup.fs_path().join("memory.stat").exists());
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

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn remove_cgroup_on_exec_v2() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    tools::create_files_cgroupv2(&root).unwrap();
    let version = CgroupVersion::V2;

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpuacct"]);
    let sub_root_p = tools::create_folder(&root, "Pantalaimon", hierarchy_cpu.clone()).unwrap();
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Pantalaimon"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
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

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn creation_of_cgroup_before_and_after_v2() {
    let version = CgroupVersion::V2;
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    assert!(tools::create_files_cgroupv2(&root).is_ok());

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(root.clone(), version, vec!["cpu", "memory"]);
    assert!(tools::create_folder(&root, "Lyra_Belacqua", hierarchy_cpu.clone()).is_ok());

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let root_clo = Arc::new(root.clone());
    let shared_root = Arc::clone(&root_clo);

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            let root_str = shared_root.file_name().unwrap().to_str().unwrap();
            let vec_child = vec![root_str, "Lyra_Belacqua", "William_Parry"];
            assert!(vec_child.iter().any(|elm| { cgroup.fs_path().ends_with(elm) }));
            assert!(cgroup.fs_path().starts_with(shared_root.to_path_buf()));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    assert!(tools::create_folder(&root, "William_Parry", hierarchy_cpu.clone()).is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(3, cpt.load(Ordering::SeqCst));
}
