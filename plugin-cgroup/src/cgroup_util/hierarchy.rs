//! Represents cgroup hierarchies.
//!
//! # Differences between cgroup v1 and cgroup v2
//!
//! In cgroup v1, each controller can get a separate hierachy.
//! In addition, one can create "named hierarchies" that have no controller.
//!
//! In cgroup v2, there is a single, unified hierarchy for the whole system.
//!
//! See `man cgroup` for more information.

use std::{
    borrow::Cow,
    fmt::Display,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::cgroup_util::mount;

use super::mount::{read_proc_mounts, Mount};

/// A control group, v1 or v2.
#[derive(Debug, Clone)]
pub struct Cgroup<'h> {
    /// Full path to the cgroup.
    sysfs_path: PathBuf,

    /// Path in the hierarchy of cgroups.
    cgroup_path: String,

    /// The hierarchy this group belongs to.
    hierarchy: &'h CgroupHierarchy,
}

/// A control group hierarchy, v1 or v2.
#[derive(Debug, Clone)]
pub struct CgroupHierarchy {
    /// The root of the hierarchy, i.e. its mount point.
    ///
    /// For cgroup v2, this is usually `/sys/fs/cgroup`.
    /// For cgroup v1, each controller can have its own hierarchy, e.g. `/sys/fs/cgroup/memory`.
    root: PathBuf,

    /// The version of the control group.
    ///
    /// Depending on the kernel parameters, cgroup v1 and cgroup v2 can coexist on the same system.
    version: CgroupVersion,

    /// List of available controllers in this hierarchy.
    available_controllers: Vec<String>,

    /// If this is a cgroup v1 and if it is named (that is, it has no controller but only exists to
    /// logically group processes, see the manual), the name.
    /// Otherwise, `None`.
    v1_name: Option<String>,
}

/// Version of the control group hierarchy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum CgroupVersion {
    V1,
    V2,
}

impl<'h> Display for Cgroup<'h> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.unique_name().as_ref())
    }
}

impl CgroupHierarchy {
    /// Analyzes the basic configuration of the hierarchy mounted at `m`.
    pub fn from_mount(m: &Mount) -> Result<Self, HierarchyError> {
        let mount_point = &m.mount_point;
        match m.fs_type.as_str() {
            "cgroup2" => {
                let root = PathBuf::from(mount_point.clone());
                let available_controllers = parse_v2_controllers(&root)?;
                Ok(Self {
                    root,
                    version: CgroupVersion::V2,
                    available_controllers,
                    v1_name: None,
                })
            }
            "cgroup" => {
                let root = PathBuf::from(mount_point.clone());
                let (available_controllers, name) = parse_v1_options(m)?;
                Ok(Self {
                    root,
                    version: CgroupVersion::V1,
                    available_controllers,
                    v1_name: name,
                })
            }
            _ => Err(HierarchyError::NotCgroupfs(PathBuf::from(mount_point))),
        }
    }

    /// Analyzes the given path as if it was a cgroup (v1 or v2) hierarchy.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use plugin_cgroup::cgroup_util::CgroupHierarchy;
    ///
    /// let h = CgroupHierarchy::from_root_path("/sys/fs/cgroup").unwrap();
    /// let cgroup_version = h.version();
    /// ```
    pub fn from_root_path(path: impl Into<PathBuf>) -> Result<Self, HierarchyError> {
        let path: PathBuf = path.into();

        // discard bad root paths
        if !path.is_absolute() {
            return Err(HierarchyError::BadRoot(path));
        }
        if let Err(e) = path.try_exists() {
            if e.kind() == ErrorKind::NotFound {
                return Err(HierarchyError::BadRoot(path));
            } else {
                return Err(HierarchyError::File(e, path));
            }
        }

        // detect the cgroup version by trying v2 first, v1 if it fails
        let (version, available_controllers, v1_name) = match parse_v2_controllers(&path) {
            Ok(controllers) => {
                // cgroups v2
                (CgroupVersion::V2, controllers, None)
            }
            Err(HierarchyError::WrongVersion) => {
                // cgroups v1
                match parse_v1_options_from_sysfs(&path) {
                    Ok((controllers, name)) => (CgroupVersion::V1, controllers, name),
                    Err(HierarchyError::WrongVersion) => return Err(HierarchyError::NotCgroupfs(path)),
                    Err(e) => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };

        Ok(Self {
            root: path,
            version,
            available_controllers,
            v1_name,
        })
    }

    /// The root path of this hierarchy.
    ///
    /// ## Differences between cgroups v1 and v2
    /// ### Cgroups v1
    /// In cgroups v1, each resource controller gets a separate hierarchy, i.e. they are mounted separately (as a `cgroup` filesystem).
    /// Multiple controllers can be mounted together, such as `cpu` and `cpuacct`.
    ///
    /// An example of a cgroup v1 hierarchy root is thus `/sys/fs/cgroup/cpu,cpuacct`.
    ///
    /// ### Cgroups v2
    /// In cgroups v2, there is a single, unified hierarchy, mounted as a `cgroup2` filesystem.
    ///
    /// An example of a cgroup v2 hierarchy root is thus `/sys/fs/cgroup`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The version of cgroups.
    pub fn version(&self) -> CgroupVersion {
        self.version
    }

    /// The cgroup controllers that are available in this hierarchy.
    ///
    /// In cgroup v2, this is extracted from `cgroup.controllers`.
    pub fn available_controllers(&self) -> &[String] {
        &self.available_controllers
    }

    /// If this is a named cgroup v1 hierarchy, returns its name.
    pub fn v1_name(&self) -> Option<&str> {
        self.v1_name.as_deref()
    }

    /// Computes the path of the cgroup in its hierarchy, that is, relative to the hierarchy root, with a leading slash `/`.
    ///
    /// Returns `None` if `sysfs_path` is not in the hierarchy root.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let hierarchy: CgroupHierarchy = todo!();
    /// let cgroup_path = PathBuf::from("/sys/fs/cgroup/system.slice/bluetooth.service");
    /// let relative = hierarchy.cgroup_relative_path(&cgroup_path);
    /// assert_eq!(relative, Some("/system.slice/bluetooth.service"))
    /// ```
    pub fn cgroup_path(&self, sysfs_path: &Path) -> Option<String> {
        let relative = sysfs_path.strip_prefix(&self.root).ok()?.to_str().unwrap();
        Some(format!("/{relative}"))
    }
}

impl<'h> Cgroup<'h> {
    pub fn new(hierarchy: &'h CgroupHierarchy, sysfs_path: PathBuf) -> Self {
        let cgroup_path = hierarchy.cgroup_path(&sysfs_path).unwrap().to_owned();
        Self {
            sysfs_path,
            cgroup_path,
            hierarchy,
        }
    }

    /// The absolute path of the cgroup in the filesystem.
    ///
    /// For example, `/sys/fs/cgroup/user.slice/me`.
    pub fn fs_path(&self) -> &Path {
        self.sysfs_path.as_path()
    }

    /// The path of the cgroup in its hierarchy.
    ///
    /// For example, `/user.slice/me`.
    pub fn cgroup_path(&self) -> &str {
        &self.cgroup_path
    }

    /// Returns the unique name of the cgroup.
    ///
    /// - In cgroup v1, this is a string of the form `cpu,cpuacct:/user.slice/me`.
    /// - In cgroup v2, the string does not depend on the controllers and is equal to the [`cgroup_path`](Self::cgroup_path).
    pub fn unique_name(&self) -> Cow<str> {
        match self.hierarchy.version() {
            CgroupVersion::V1 => {
                // there can be multiple cgroup hierarchies, we have to say in which one we are
                let controllers = self.hierarchy.available_controllers().join(",");
                let cgroup = &self.cgroup_path;
                Cow::Owned(format!("{controllers}:{cgroup}"))
            }
            CgroupVersion::V2 => {
                // there is a single unified cgroup hierarchy, no need to include that information
                Cow::Borrowed(&self.cgroup_path)
            }
        }
    }

    /// A reference to the cgroup hierarchy this group belongs to.
    pub fn hierarchy(&self) -> &CgroupHierarchy {
        self.hierarchy
    }
}

fn parse_v2_controllers(cgroup_root: &Path) -> Result<Vec<String>, HierarchyError> {
    let controller_file = cgroup_root.join("cgroup.controllers");
    match std::fs::read_to_string(&controller_file) {
        Ok(content) => Ok(content.split(' ').map(|c| c.to_string()).collect()),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(HierarchyError::WrongVersion),
        Err(err) => Err(HierarchyError::File(err, controller_file)),
    }
}

fn parse_v1_options(cgroup_mount: &Mount) -> Result<(Vec<String>, Option<String>), HierarchyError> {
    let options = &cgroup_mount.mount_options;
    let no_controller = options.iter().any(|o| *o == "none");
    let hierarchy_name = options.iter().find_map(|o| o.strip_prefix("name="));
    if let Some(name) = hierarchy_name {
        debug_assert!(no_controller);
        Ok((vec![], Some(name.to_owned())))
    } else {
        Ok((options.to_owned(), None))
    }
}

fn parse_v1_options_from_sysfs(cgroup_root: &Path) -> Result<(Vec<String>, Option<String>), HierarchyError> {
    let v1_agent = cgroup_root.join("release_agent");
    if !v1_agent.try_exists().map_err(|e| HierarchyError::File(e, v1_agent))? {
        // `release_agent` only exists in the root directory of each cgroup hierarchy (see man cgroups)
        return Err(HierarchyError::WrongVersion);
    }

    let cgroup_root = cgroup_root.to_str().unwrap();

    // The cgroup v1 hierarchy does not seem to provide virtual files for the list of available controllers.
    // Furthermore, there can be "named hierarchies", which have no controller but an arbitrary name.
    // => use /proc/mounts to get the information we need

    let mounts = read_proc_mounts().map_err(HierarchyError::BadMounts)?;
    let this_mount = mounts
        .into_iter()
        .find(|m| m.mount_point == cgroup_root)
        .ok_or_else(|| HierarchyError::MountNotFound(cgroup_root.to_owned()))?;
    parse_v1_options(&this_mount)
}

#[derive(Debug, Error)]
pub enum HierarchyError {
    #[error("wrong cgroup version")]
    WrongVersion,
    #[error("unexpected IO error on {1:?}")]
    File(#[source] io::Error, PathBuf),
    #[error("{0} not found in /proc/mounts")]
    MountNotFound(String),
    #[error("failed to analyse /proc/mounts")]
    BadMounts(#[source] mount::ReadError),
    #[error("{0} is not a valid hierarchy root: the path should be absolute and should exist")]
    BadRoot(PathBuf),
    #[error("{0:?} is not a cgroup filesystem")]
    NotCgroupfs(PathBuf),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::cgroup_util::{
        hierarchy::{CgroupHierarchy, CgroupVersion, HierarchyError},
        mount::Mount,
        Cgroup,
    };

    #[test]
    fn cgroup_properties_v1() {
        let h = CgroupHierarchy {
            root: PathBuf::from("/some/root"),
            version: CgroupVersion::V1,
            available_controllers: vec![String::from("cpu"), String::from("cpuacct")],
            v1_name: None,
        };
        assert_eq!(h.cgroup_path(&PathBuf::from("/some/root")).unwrap(), "/");
        assert_eq!(h.cgroup_path(&PathBuf::from("/some/root/")).unwrap(), "/");
        assert_eq!(h.cgroup_path(&PathBuf::from("/a/b")), None);

        let sysfs_path = PathBuf::from("/some/root/a.slice/me");
        let cgroup = Cgroup::new(&h, sysfs_path.clone());
        assert_eq!(cgroup.fs_path(), sysfs_path);
        assert_eq!(cgroup.cgroup_path(), "/a.slice/me");
        assert_eq!(cgroup.unique_name(), "cpu,cpuacct:/a.slice/me");
        assert_eq!(cgroup.hierarchy().version(), CgroupVersion::V1);
    }

    #[test]
    fn cgroup_properties_v2() {
        let h = CgroupHierarchy {
            root: PathBuf::from("/some/root"),
            version: CgroupVersion::V2,
            available_controllers: vec![String::from("cpu"), String::from("cpuacct")],
            v1_name: None,
        };
        assert_eq!(h.cgroup_path(&PathBuf::from("/some/root")).unwrap(), "/");
        assert_eq!(h.cgroup_path(&PathBuf::from("/some/root/")).unwrap(), "/");
        assert_eq!(h.cgroup_path(&PathBuf::from("/a/b")), None);

        let sysfs_path = PathBuf::from("/some/root/a.slice/me");
        let cgroup = Cgroup::new(&h, sysfs_path.clone());
        assert_eq!(cgroup.fs_path(), sysfs_path);
        assert_eq!(cgroup.cgroup_path(), "/a.slice/me");
        assert_eq!(cgroup.unique_name(), "/a.slice/me");
        assert_eq!(cgroup.hierarchy().version(), CgroupVersion::V2);
    }

    #[test]
    fn bad_hierarchy_from_mount() {
        let mount = Mount::parse("tmpfs /tmp tmpfs rw 0 0").unwrap();
        let h = CgroupHierarchy::from_mount(&mount);
        assert!(matches!(h, Err(HierarchyError::NotCgroupfs(p)) if p == PathBuf::from("/tmp")));
    }

    #[test]
    fn bad_hierarchy_from_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let h = CgroupHierarchy::from_root_path(&root);
        assert!(
            matches!(&h, Err(HierarchyError::NotCgroupfs(p)) if p == root),
            "unexpected result {h:?}"
        );

        let does_not_exist = tmp.path().join("titebouille");
        let h = CgroupHierarchy::from_root_path(&does_not_exist);
        assert!(
            matches!(&h, Err(HierarchyError::NotCgroupfs(p)) if p == &does_not_exist),
            "unexpected result {h:?}"
        );
    }
}
