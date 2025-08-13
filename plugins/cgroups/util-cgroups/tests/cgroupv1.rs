use std::fs::{self};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread;
use std::time::Duration;
use tempfile::tempdir;
use util_cgroups::detect::{ClosureCallbacks, Config, callback};
use util_cgroups::hierarchy::CgroupVersion;
use util_cgroups::{CgroupDetector, CgroupHierarchy};

mod tools;

#[test]
fn one_cgroup_created_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    let path_mem = root.join("memory");
    //Creation of CgroupHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);
    // Creation of cgroup
    assert!(tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&root, "memory", hierarchy_mem.clone()).is_ok());

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out1 = Arc::clone(&cpt);
    let ac_out2 = Arc::clone(&cpt);

    // Detection of Cgroup
    let config1 = Config::default();
    let config2 = Config::default();
    let f1 = callback(move |cgroups| {
        assert_eq!(cgroups.len(), 1);
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, Ordering::SeqCst);
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
            ac_out2.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_mem, config2, handler2);
    assert!(detector.is_ok());

    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn one_cgroup_created_after_v1() {
    env_logger::init_from_env(env_logger::Env::default());
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgroupHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu, version, vec!["cpuacct"]);
    let res_cpu = tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).unwrap();
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    let _res_cpu = tools::create_folder(&res_cpu, "toto", hierarchy_cpu.clone());
    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn severale_cgroup_created_after_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgroupHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    assert!(tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    assert!(tools::create_folder(&path_cpu, "Christopher_Eccleston", hierarchy_cpu.clone()).is_ok());

    assert!(tools::create_folder(&cpu_root, "Rose_Tyler", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&cpu_root, "Jack_Harkness", hierarchy_cpu.clone()).is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(4, cpt.load(Ordering::SeqCst));
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

    assert!(tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&root1, "memory", hierarchy_mem.clone()).is_ok());

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out1 = Arc::clone(&cpt);
    let ac_out2 = Arc::clone(&cpt);

    let config1 = Config::default();
    let config2 = Config::default();

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, Ordering::SeqCst);
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
            ac_out2.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let _detector = CgroupDetector::new(hierarchy_mem, config2, handler2);

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
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

    let res_cpu = tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).expect("Can't create a folder");
    let res_mem = tools::create_folder(&root, "memory", hierarchy_mem.clone()).expect("Can't create a folder");
    assert!(tools::create_folder(&res_cpu, "dalek", hierarchy_cpu.clone()).is_ok());
    assert!(tools::create_folder(&res_mem, "dalek", hierarchy_mem.clone()).is_ok());
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out1 = Arc::clone(&cpt);
    let ac_out2 = Arc::clone(&cpt);

    // Detection of Cgroup
    let config1 = Config::default();
    let config2 = Config::default();
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out1.fetch_add(1, Ordering::SeqCst);
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
            ac_out2.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    });
    let handler2 = ClosureCallbacks {
        on_cgroups_created: f3,
        on_cgroups_removed: f2,
    };
    let detector = CgroupDetector::new(hierarchy_mem, config2, handler2);
    assert!(detector.is_ok());

    thread::sleep(tools::WAITING);
    assert_eq!(4, cpt.load(Ordering::SeqCst));
}

#[test]
fn cgroup_missing_element_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");
    //Creation of CgroupHierarchy
    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    assert!(tools::create_folder(&root, "cpuacct", hierarchy_cpu.clone()).is_ok());
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, Ordering::SeqCst);
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
    assert!(tools::is_accessible_dir(&dir_path).unwrap());
    let _file_path = tools::create_files_cgroupv1(&dir_path, hierarchy_cpu).unwrap();

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn remove_cgroup_on_exec_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;

    let path_mem = root.join("memory");

    let hierarchy_mem = CgroupHierarchy::manually_unchecked(path_mem, version, vec!["memory"]);
    let res_mem = tools::create_folder(&root, "memory", hierarchy_mem.clone()).expect("Can't create a folder");
    let res_mem = tools::create_folder(&res_mem, "dalek", hierarchy_mem.clone()).unwrap();
    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);
    let config = Config::default();

    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "memory"));
            ac_out.fetch_add(1, Ordering::SeqCst);
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

    thread::sleep(tools::WAITING);
    assert_eq!(2, cpt.load(Ordering::SeqCst));
}

#[test]
fn creation_of_cgroup_before_and_after_v1() {
    let root_unwraped = tempdir().unwrap();
    let root = root_unwraped.path().to_path_buf();
    let version = CgroupVersion::V1;
    // Create path before
    let path_cpu = root.join("cpuacct");

    let cpt: Arc<AtomicU8> = Arc::new(AtomicU8::new(0));
    let ac_out = Arc::clone(&cpt);

    let hierarchy_cpu = CgroupHierarchy::manually_unchecked(path_cpu.clone(), version, vec!["cpuacct"]);
    let cpu_root = path_cpu.clone().join("Peter_Capaldi");
    assert!(tools::create_folder(&path_cpu, "Peter_Capaldi", hierarchy_cpu.clone()).is_ok());
    let _path_clara = cpu_root.clone().join("Clara_Oswald");
    assert!(tools::create_folder(&cpu_root, "Clara_Oswald", hierarchy_cpu.clone()).is_ok());

    // Detection of Cgroup
    let config = Config {
        v1_refresh_interval: Duration::from_millis(10),
        force_polling: false,
    };
    let f1 = callback(move |cgroups| {
        for cgroup in cgroups {
            assert!(cgroup.fs_path().iter().any(|part| part.to_os_string() == "cpuacct"));
            ac_out.fetch_add(1, Ordering::SeqCst);
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

    assert!(tools::create_folder(&cpu_root, "River_Song", hierarchy_cpu.clone()).is_ok());
    thread::sleep(tools::WAITING);
    assert_eq!(4, cpt.load(Ordering::SeqCst));
}
