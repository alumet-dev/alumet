// Prevent compiling outside of Linux: cgroups only exist on Linux, and we rely on Linux mechanisms like /proc/mounts, inotify and epoll.
#[cfg(not(target_os = "linux"))]
compile_error!("only Linux is supported");

pub mod detect;
pub mod file_watch;
pub mod hierarchy;
pub mod measure;
pub mod mount_wait;

// re-exports
pub use detect::CgroupDetector;
pub use hierarchy::{Cgroup, CgroupHierarchy, CgroupVersion};
pub use mount_wait::CgroupMountWait;
