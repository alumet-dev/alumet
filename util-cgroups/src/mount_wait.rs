//! Wait for the cgroupfs to be mounted.

use mount_watcher::{mount::LinuxMount, MountWatcher, WatchControl};
use std::{ops::ControlFlow, time::Duration};

use crate::CgroupVersion;

use super::hierarchy::CgroupHierarchy;

/// `CgroupMountWait` represents a handle to a background thread that waits for a cgroup filesystem to be mounted.
///
/// When the `CgroupMountWait` is dropped, the background thread is stopped.
///
/// # Example
///
/// ```
/// use util_cgroups::mount_wait::CgroupMountWait;
///
/// let wait = CgroupMountWait::new(None, |hierarchies| {
///     for h in hierarchies {
///         todo!()
///     }
///     Ok(())
/// });
/// ```
pub struct CgroupMountWait {
    watcher: Option<MountWatcher>,
}

/// A callback that is called when new cgroup filesystems are detected by a [`CgroupMountWait`].
pub trait Callback: Send {
    /// Called when new cgroup filesystems are mounted.
    ///
    /// With cgroup v2, only one cgroupfs can be mounted in the system.
    /// However, with cgroup v1, there are multiple cgroupfs (one per hierarchy), each with their own controller(s).
    ///
    /// # Return value
    /// Return `ControlFlow::Continue(())` to keep being notified for more cgroup hierarchies, and `ControlFlow::Break(())` to stop the wait.
    fn on_cgroupfs_mounted(&mut self, hierarchies: Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>>;
}

impl<F: FnMut(Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>> + Send> Callback for F {
    fn on_cgroupfs_mounted(&mut self, hierarchies: Vec<CgroupHierarchy>) -> anyhow::Result<ControlFlow<()>> {
        self(hierarchies)
    }
}

impl CgroupMountWait {
    /// Waits for a cgroupfs to be mounted and executes the given `callback` when it occurs.
    ///
    /// The trigger decides whether the wait should continue or not.
    pub fn new(coalesce_v1: Option<Duration>, callback: impl Callback + 'static) -> anyhow::Result<Self> {
        let watcher = prepare_watcher(callback, coalesce_v1)?;
        Ok(Self { watcher: Some(watcher) })
    }

    /// Stops the waiting thread and wait for it to terminate.
    ///
    /// # Errors
    /// If the thread has panicked, an error is returned with the panic payload.
    pub fn stop_and_join(mut self) -> std::thread::Result<()> {
        let watcher = self.watcher.take().unwrap();
        watcher.stop();
        watcher.join()
    }
}

impl Drop for CgroupMountWait {
    fn drop(&mut self) {
        if let Some(w) = self.watcher.take() {
            w.stop(); // just set the flag
        }
    }
}

/// Prepare a `MountWatcher` that triggers the `callback` when new cgroupfs are mounted.
fn prepare_watcher(
    mut callback: impl Callback + 'static,
    coalesce_v1: Option<Duration>,
) -> anyhow::Result<MountWatcher> {
    let watcher = MountWatcher::new(move |event| {
        // find the cgroup filesystems, if any
        let new_cgroupfs = extract_cgroup_hierarchies(&event.mounted);

        // coalesce cgroupv1 events, because multiple cgroupfs v1 are mounted in a short period of time, and we want them all
        if let Some(delay) = coalesce_v1 {
            if !event.coalesced && new_cgroupfs.iter().any(|c| c.version() == CgroupVersion::V1) {
                return WatchControl::Coalesce { delay };
            }
        }

        // call the user-provided function
        if !new_cgroupfs.is_empty() {
            match callback.on_cgroupfs_mounted(new_cgroupfs) {
                Ok(ControlFlow::Continue(())) => return WatchControl::Continue,
                Ok(ControlFlow::Break(())) => return WatchControl::Stop,
                Err(err) => log::error!("error in callback: {err:?}"),
            };
        }

        // no cgroups, continue
        WatchControl::Continue
    })?;
    Ok(watcher)
}

/// For each mount that correspond to a cgoup filesystem (v1 or v2), builds a [`CgroupHierarchy`].
fn extract_cgroup_hierarchies(mounts: &[LinuxMount]) -> Vec<CgroupHierarchy> {
    mounts
        .iter()
        .filter_map(|m| match CgroupHierarchy::from_mount(m) {
            Ok(h) => Some(h),
            Err(e) => {
                log::warn!(
                    "{m:?} appears to be a cgroup, but I could not construct a CgroupHierarchy structure from it: {e:#}"
                );
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use mount_watcher::mount::LinuxMount;
    use pretty_assertions::assert_eq;
    use std::io;

    use super::super::hierarchy::CgroupVersion;
    use super::extract_cgroup_hierarchies;

    fn vec_str(values: &[&str]) -> Vec<String> {
        values.into_iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn extraction_cgroupv2() -> io::Result<()> {
        let tmp = tempfile::tempdir()?;
        let fake_cgroup_root = tmp.path();
        let fake_controllers = fake_cgroup_root.join("cgroup.controllers");
        std::fs::write(fake_controllers, "cpuset cpu io memory hugetlb pids")?;

        let mounts = vec![
            LinuxMount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
            LinuxMount {
                spec: String::from("cgroup2"),
                mount_point: fake_cgroup_root.to_str().unwrap().to_owned(),
                fs_type: String::from("cgroup2"),
                mount_options: vec_str(&[
                    "rw",
                    "nosuid",
                    "nodev",
                    "noexec",
                    "relatime",
                    "nsdelegate",
                    "memory_recursiveprot",
                ]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
        ];

        let cgroups = extract_cgroup_hierarchies(&mounts);
        let cgroup = &cgroups[0];
        assert_eq!(cgroup.root(), fake_cgroup_root);
        assert_eq!(cgroup.version(), CgroupVersion::V2);
        assert_eq!(
            cgroup.available_controllers(),
            vec!["cpuset", "cpu", "io", "memory", "hugetlb", "pids"]
        );
        Ok(())
    }

    #[test]
    fn extraction_cgroupv1() -> io::Result<()> {
        let tmp = tempfile::tempdir()?;
        let fake_cgroup_root = tmp.path();

        let mounts = vec![
            LinuxMount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
            LinuxMount {
                spec: String::from("cgroup"),
                mount_point: fake_cgroup_root.to_str().unwrap().to_owned(),
                fs_type: String::from("cgroup"),
                mount_options: vec_str(&["cpu", "cpuacct"]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
        ];

        let cgroups = extract_cgroup_hierarchies(&mounts);
        let cgroup = &cgroups[0];
        assert_eq!(cgroup.root(), fake_cgroup_root);
        assert_eq!(cgroup.version(), CgroupVersion::V1);
        assert_eq!(cgroup.available_controllers(), vec!["cpu", "cpuacct"]);
        Ok(())
    }

    #[test]
    fn extraction_no_cgroup() {
        let mounts = vec![
            LinuxMount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
            LinuxMount {
                spec: String::from("/dev/nvme0n1p1"),
                mount_point: String::from("/boot/efi"),
                fs_type: String::from("vfat"),
                mount_options: vec_str(&["rw", "relatime", "errors=remount-ro"]),
                dump_fs_freq: 0,
                fsck_fs_passno: 0,
            },
        ];

        let cgroups = extract_cgroup_hierarchies(&mounts);
        assert!(cgroups.is_empty());
    }
}
