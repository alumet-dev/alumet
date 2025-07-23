use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

use util_cgroups::{
    CgroupDetector, CgroupHierarchy, CgroupVersion,
    detect::{ClosureCallbacks, Config, callback},
    hierarchy,
};

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
        on_cgroups_removed: callback(|cgroups| todo!()),
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
        on_cgroups_removed: callback(|cgroups| todo!()),
    };
    let cgroup_detector = CgroupDetector::new(hierarchy, config, handler);
    assert!(cgroup_detector.is_err());

    Ok(())
}
