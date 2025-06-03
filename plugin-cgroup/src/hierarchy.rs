use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use thiserror::Error;

use crate::mount_wait::Mount;

pub struct Cgroup<'h> {
    /// Full path to the cgroup.
    sysfs_path: PathBuf,

    /// Path in the hierarchy of cgroups.
    relative_path: String,

    hierarchy: &'h CgroupHierarchy,
}

#[derive(Debug, Clone)]
pub struct CgroupHierarchy {
    root: PathBuf,
    version: CgroupVersion,
    available_controllers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum CgroupVersion {
    V1,
    V2,
}

impl CgroupHierarchy {
    pub(crate) fn from_mount(m: &Mount) -> anyhow::Result<Self> {
        match m.fs_type.as_str() {
            "cgroup2" => {
                let root = PathBuf::from(m.mount_point.clone());
                let available_controllers = parse_v2_controllers(&root)?;
                Ok(Self {
                    root,
                    version: CgroupVersion::V2,
                    available_controllers,
                })
            }
            "cgroup" => {
                let root = PathBuf::from(m.mount_point.clone());
                let available_controllers = m.mount_options.clone();
                Ok(Self {
                    root,
                    version: CgroupVersion::V1,
                    available_controllers,
                })
            }
            _ => Err(anyhow!("todo")),
        }
    }

    pub fn new_at(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path: PathBuf = path.into();
        let (version, available_controllers) = match parse_v2_controllers(&path) {
            Ok(controllers) => {
                // cgroups v2
                (CgroupVersion::V2, controllers)
            }
            Err(ParseError::WrongVersion) => {
                // cgroups v1
                match guess_v1_controllers(&path) {
                    Ok(controllers) => (CgroupVersion::V1, controllers),
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

    pub fn available_controllers(&self) -> &[String] {
        &self.available_controllers
    }
}

impl<'h> Cgroup<'h> {
    pub fn new(hierarchy: &'h CgroupHierarchy, sysfs_path: PathBuf) -> Self {
        let relative_path = sysfs_path
            .strip_prefix(&hierarchy.root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
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
        Err(err) => Err(ParseError::Other(err)),
    }
}

fn guess_v1_controllers(cgroup_root: &Path) -> Result<Vec<String>, ParseError> {
    if !cgroup_root
        .join("release_agent")
        .try_exists()
        .map_err(ParseError::Other)?
    {
        // `release_agent` only exists in the root directory of each cgroup hierarchy (see man cgroups)
        return Err(ParseError::WrongVersion);
    }

    // FIXME: this only works for regular cgroup hierachies, not for named ones.
    // See "Cgroup v1 named hierarchies" in man cgroups.
    let mount_point_name = cgroup_root.file_name().ok_or(ParseError::WrongVersion)?;
    Ok(mount_point_name
        .to_str()
        .unwrap()
        .split(',')
        .map(|c| c.to_string())
        .collect())
}

#[derive(Debug, Error)]
#[error("failed to parse cgroup controllers")]
enum ParseError {
    WrongVersion,
    Other(std::io::Error),
}
