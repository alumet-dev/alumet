//! Utilities to use control groups, v1 and v2.

pub mod detect;
pub mod hierarchy;
pub mod mount;
pub mod mount_wait;

// re-exports
pub use detect::CgroupDetector;
pub use hierarchy::{Cgroup, CgroupHierarchy};
pub use mount_wait::MountWait;
