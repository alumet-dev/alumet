//! Wait for the cgroupfs to be mounted.

use std::{
    fs::File,
    io::{ErrorKind, Read},
    os::fd::AsRawFd,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

use anyhow::Context;
use mio::{unix::SourceFd, Events, Interest, Poll, Token};

use super::{
    hierarchy::CgroupHierarchy,
    mount::{parse_proc_mounts, Mount},
};

/// `MountWait` represents a handle to a background thread that waits for a cgroup filesystem to be mounted.
///
/// When the `MountWait` is dropped, the background thread is stopped.
/// You can also call [`stop`](Self::stop).
///
/// # Example
///
/// ```
/// use util_cgroups::mount_wait::MountWait;
///
/// let wait = MountWait::new(|hierarchies| {
///     for h in hierarchies {
///         todo!()
///     }
///     Ok(())
/// });
/// ```
pub struct MountWait {
    thread_handle: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
}

impl MountWait {
    /// Waits for a cgroupfs to be mounted and executes the given `callback` when it occurs.
    pub fn new(
        callback: impl FnMut(Vec<CgroupHierarchy>) -> anyhow::Result<()> + Send + 'static,
    ) -> anyhow::Result<Self> {
        wait_for_cgroupfs(callback)
    }

    // TODO add a "manual" constructor that uses a socket to trigger epoll?
    // Some additional modifications are needed, because File::open won't work on a socket.
    // TODO add a delay for cgroup v1 to trigger the callback with all the cgroups at once?

    /// Stops the waiting thread and wait for it to terminate.
    ///
    /// # Errors
    /// If the thread has panicked, an error is returned with the panic payload.
    pub fn stop(mut self) -> std::thread::Result<()> {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.thread_handle.take().unwrap().join()
    }
}

impl Drop for MountWait {
    fn drop(&mut self) {
        if self.thread_handle.is_some() {
            self.stop_flag.store(true, Ordering::Relaxed);
        }
    }
}

const SINGLE_TOKEN: Token = Token(0);
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
const PROC_MOUNTS_PATH: &str = "/proc/mounts";

/// Starts a background thread that uses [`mio::poll`] (backed by `epoll`) to detect changes to the mounted filesystem.
fn wait_for_cgroupfs(
    mut callback: impl FnMut(Vec<CgroupHierarchy>) -> anyhow::Result<()> + Send + 'static,
) -> anyhow::Result<MountWait> {
    // Open the file that contains info about the mounted filesystems.
    let file = File::open(PROC_MOUNTS_PATH).with_context(|| format!("failed to open {PROC_MOUNTS_PATH}"))?;
    let fd = file.as_raw_fd();
    let mut fd = SourceFd(&fd);

    // Prepare epoll.
    // According to `man proc_mounts`,  a filesystem mount or unmount causes
    // `poll` and `epoll_wait` to mark the file as having a PRIORITY event.
    let mut poll = Poll::new().context("poll init failed")?;
    poll.registry()
        .register(&mut fd, SINGLE_TOKEN, Interest::PRIORITY)
        .with_context(|| format!("poll registration of {PROC_MOUNTS_PATH:?} failed"))?;

    // Keep a boolean to stop the thread from the outside.
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_thread = stop_flag.clone();

    // Declare the polling loop separately to handle errors in a nicer way.
    let poll_loop = move || -> anyhow::Result<()> {
        let mut events = Events::with_capacity(8);
        let mut finder = CgroupMountFinder {
            file,
            content_buf: String::with_capacity(8196),
            mounts_buf: Vec::with_capacity(1),
        };

        // While we were setting up epoll, the cgroupfs may have been mounted.
        // Check that here to avoid any miss.
        if let Some(cgroups) = finder.find_cgroupfs_in_mounts().context("mount analysis failed")? {
            callback(cgroups).context("error in callback")?;
        }

        loop {
            let callback = &mut callback;
            let poll_res = poll.poll(&mut events, Some(POLL_TIMEOUT));
            if let Err(e) = poll_res {
                if e.kind() == ErrorKind::Interrupted {
                    continue; // retry
                } else {
                    return Err(e.into()); // propagate error
                }
            }

            // Call next() because we are not interested in each individual event.
            // If the timeout elapses, the event list is empty.
            if let Some(event) = events.iter().next() {
                log::debug!("event on /proc/mounts: {event:?}");

                // parse mount file
                if let Some(cgroups) = finder.find_cgroupfs_in_mounts()? {
                    callback(cgroups).context("error in callback")?;
                    break;
                }
            }
            if stop_flag_thread.load(Ordering::Relaxed) {
                break;
            }
        }
        Ok(())
    };
    // Spawn a thread.
    let thread_handle = std::thread::spawn(move || {
        if let Err(e) = poll_loop() {
            log::error!("error in poll loop: {e:?}");
        }
    });
    // Return a structure that will stop the polling when dropped.
    Ok(MountWait {
        thread_handle: Some(thread_handle),
        stop_flag,
    })
}

struct CgroupMountFinder {
    /// `/proc/mounts`, opened
    file: File,
    content_buf: String,
    mounts_buf: Vec<Mount>,
}

impl CgroupMountFinder {
    /// Finds all `cgroup` and `cgroup2` mounts in `/proc/mounts`.
    fn find_cgroupfs_in_mounts(&mut self) -> anyhow::Result<Option<Vec<CgroupHierarchy>>> {
        // parse mount file
        self.file.read_to_string(&mut self.content_buf)?;
        parse_proc_mounts(&self.content_buf, &mut self.mounts_buf)?;

        // Is my cgroupfs mounted?
        let cgroup_filesystems = extract_cgroup_hierarchies(&self.mounts_buf);
        if cgroup_filesystems.is_empty() {
            Ok(None)
        } else {
            Ok(Some(cgroup_filesystems))
        }
    }
}

/// For each mount that correspond to a cgoup filesystem (v1 or v2), builds a [`CgroupHierarchy`].
fn extract_cgroup_hierarchies(mounts: &[Mount]) -> Vec<CgroupHierarchy> {
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
    use pretty_assertions::assert_eq;
    use std::io;

    use super::super::{hierarchy::CgroupVersion, mount::Mount};
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
            Mount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
            },
            Mount {
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
            Mount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
            },
            Mount {
                spec: String::from("cgroup"),
                mount_point: fake_cgroup_root.to_str().unwrap().to_owned(),
                fs_type: String::from("cgroup"),
                mount_options: vec_str(&["cpu", "cpuacct"]),
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
            Mount {
                spec: String::from("sysfs"),
                mount_point: String::from("/sys"),
                fs_type: String::from("sysfs"),
                mount_options: vec_str(&["rw", "nosuid", "nodev", "noexec", "relatime"]),
            },
            Mount {
                spec: String::from("/dev/nvme0n1p1"),
                mount_point: String::from("/boot/efi"),
                fs_type: String::from("vfat"),
                mount_options: vec_str(&["rw", "relatime", "errors=remount-ro"]),
            },
        ];

        let cgroups = extract_cgroup_hierarchies(&mounts);
        assert!(cgroups.is_empty());
    }
}
