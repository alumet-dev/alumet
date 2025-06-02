use std::{
    fmt::Display,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context};
use thiserror::Error;

use super::mount::{Mount, read_proc_mounts};

/// A control group, v1 or v2.
#[derive(Debug, Clone)]
pub struct Cgroup<'h> {
    /// Full path to the cgroup.
    sysfs_path: PathBuf,

    /// Path in the hierarchy of cgroups.
    relative_path: String,

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
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum CgroupVersion {
    V1,
    V2,
}

impl<'h> Display for Cgroup<'h> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} in {}", self.relative_path, self.hierarchy.root().display())
    }
}

impl CgroupHierarchy {
    /// Analyzes the basic configuration of the hierarchy mounted at `m`.
    pub(crate) fn from_mount(m: &Mount) -> anyhow::Result<Self> {
        match m.fs_type.as_str() {
            "cgroup2" => {
                let root = PathBuf::from(m.mount_point.clone());
                let available_controllers = parse_v2_controllers(&root)?;
                Ok(Self {
                    root,
                    version: CgroupVersion::V2,
                    available_controllers,
                    v1_name: None,
                })
            }
            "cgroup" => {
                let root = PathBuf::from(m.mount_point.clone());
                let (available_controllers, name) = parse_v1_options(m)?;
                Ok(Self {
                    root,
                    version: CgroupVersion::V1,
                    available_controllers,
                    v1_name: name,
                })
            }
            _ => Err(anyhow!("todo")),
        }
    }

    /// Attempts to analyze the given path as if it was a cgroup (v1 or v2) hierarchy.
    ///
    /// # Limitations
    ///
    /// This function works well with cgroup v2, because every information we need is available
    /// in the cgroup filesystem.
    /// However, it is more complicated for cgroup v1, where we apply a reasonable "guess".
    pub fn from_root_path(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path: PathBuf = path.into();
        let (version, available_controllers, v1_name) = match parse_v2_controllers(&path) {
            Ok(controllers) => {
                // cgroups v2
                (CgroupVersion::V2, controllers, None)
            }
            Err(ParseError::WrongVersion) => {
                // cgroups v1
                match parse_v1_options_from_sysfs(&path) {
                    Ok((controllers, name)) => (CgroupVersion::V1, controllers, name),
                    Err(ParseError::WrongVersion) => {
                        return Err(anyhow!("{path:?} is neither a cgroup v1 nor cgroup v2 hierarchy"));
                    }
                    Err(ParseError::Other(err)) => return Err(err.into()),
                }
            }
            Err(ParseError::Other(err)) => return Err(err.into()),
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

    /// Returns the path of the cgroup, relative to the hierarchy root.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # let hierarchy: CgroupHierarchy = todo!();
    /// let cgroup_path = PathBuf::from("/sys/fs/cgroup/system.slice/bluetooth.service");
    /// let relative = hierarchy.cgroup_relative_path(&cgroup_path);
    /// assert_eq!(relative, Some("system.slice/bluetooth.service"))
    /// ```
    pub fn cgroup_relative_path<'b>(&self, sysfs_path: &'b Path) -> Option<&'b str> {
        Some(sysfs_path.strip_prefix(&self.root).ok()?.to_str().unwrap())
    }
}

impl<'h> Cgroup<'h> {
    pub fn new(hierarchy: &'h CgroupHierarchy, sysfs_path: PathBuf) -> Self {
        let relative_path = hierarchy.cgroup_relative_path(&sysfs_path).unwrap().to_owned();
        Self {
            sysfs_path,
            relative_path,
            hierarchy,
        }
    }

    pub fn fs_path(&self) -> &Path {
        self.sysfs_path.as_path()
    }

    pub fn cgroup_path(&self) -> &str {
        &self.relative_path
    }

    pub fn hierarchy(&self) -> &CgroupHierarchy {
        self.hierarchy
    }
}

fn parse_v2_controllers(cgroup_root: &Path) -> Result<Vec<String>, ParseError> {
    let controller_files = cgroup_root.join("cgroup.controllers");
    match std::fs::read_to_string(controller_files) {
        Ok(content) => Ok(content.split(' ').map(|c| c.to_string()).collect()),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(ParseError::WrongVersion),
        Err(err) => Err(ParseError::Other(err.into())),
    }
}

fn parse_v1_options(cgroup_mount: &Mount) -> Result<(Vec<String>, Option<String>), ParseError> {
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

fn parse_v1_options_from_sysfs(cgroup_root: &Path) -> Result<(Vec<String>, Option<String>), ParseError> {
    if !cgroup_root
        .join("release_agent")
        .try_exists()
        .map_err(|e| ParseError::Other(e.into()))?
    {
        // `release_agent` only exists in the root directory of each cgroup hierarchy (see man cgroups)
        return Err(ParseError::WrongVersion.into());
    }

    let cgroup_root = cgroup_root.to_str().unwrap();

    // The cgroup v1 hierarchy does not seem to provide virtual files for the list of available controllers.
    // Furthermore, there can be "named hierarchies", which have no controller but an arbitrary name.
    // => use /proc/mounts to get the information we need

    let mounts = read_proc_mounts().map_err(|e| ParseError::Other(e.into()))?;
    let this_mount = mounts
        .into_iter()
        .find(|m| m.mount_point == cgroup_root)
        .with_context(|| format!("could not find {cgroup_root} in /proc/mounts"))
        .map_err(|e| ParseError::Other(e))?;
    parse_v1_options(&this_mount)
}

#[derive(Debug, Error)]
#[error("failed to parse cgroup controllers")]
enum ParseError {
    WrongVersion,
    Other(#[source] anyhow::Error),
}
