//! Resources (measurement perimeter).
//!
//! In Alumet, a "resource" represents a piece of hardware or software for which measurements can be obtained.
//! In other words, a resource gives the perimeter of a measurement.
//! Are we measuring the energy consumption of a GPU, of the whole machine or of a process of our operating system?
//!
//! The largest perimeter is "the whole machine", represented by [`Resource::LocalMachine`].
//! Therefore, if you work in a distributed environment, the resource id is not enough to identify what is being measured.
//! You should add more information to your data, such as the hostname.
//!
//! # Measurement points and resources
//!
//! To create a measurement point for a given resource, use
//! the [`Resource`] enum to provide a unique resource identifier.
//! Here is an example of a measurement point associated with the first CPU package (id "0").
//! ```no_run
//! use alumet::measurement::{MeasurementPoint, Timestamp};
//! use alumet::resources::{Resource, ResourceConsumer};
//! #
//! # use alumet::metrics::TypedMetricId;
//! # let timestamp = Timestamp::now();
//! # let metric_id: TypedMetricId<u64> = todo!();
//! # let measurement_value = 0;
//!
//! let measure = MeasurementPoint::new(
//!     timestamp,
//!     metric_id,
//!     Resource::CpuPackage { id: 0 },
//!     ResourceConsumer::LocalMachine,
//!     measurement_value
//! );
//! ```
//!
//! Unlike metrics and units, resources are not registered in a global registry,
//! but created each time they are needed.

use std::{borrow::Cow, fmt};

/// Alias to a static cow. It helps to avoid the allocation of Strings.
pub type StrCow = Cow<'static, str>;

/// Hardware or software entity that can be measured.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum Resource {
    /// The whole local machine, for instance the whole physical server.
    LocalMachine,
    /// A physical CPU package (which is not always the same as a NUMA node).
    CpuPackage { id: u32 },
    /// A CPU core.
    CpuCore { id: u32 },
    /// The RAM attached to a CPU package.
    Dram { pkg_id: u32 },
    /// A dedicated GPU.
    Gpu { bus_id: StrCow },
    /// A custom resource.
    Custom { kind: StrCow, id: StrCow },
}

/// Something that uses a [`Resource`].
///
/// Consumers are useful to track the consumption of resources with several levels of granularity.
/// For instance, the memory consumption (`Dram` resource) can be measured at the OS level
/// (total memory consumption, with consumer `LocalMachine`), or at the process level
/// (process memory consumption, with consumer `Process { pid }`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(C)]
pub enum ResourceConsumer {
    /// The whole local machine.
    ///
    /// You can use this when there is no "consumer" of your resource, for instance when reporting
    /// the temperature of a particular hardware component.
    LocalMachine,
    /// A process at the OS level.
    Process { pid: u32 },
    /// A control group, often abbreviated cgroup.
    ControlGroup { path: StrCow },
    /// A custom resource consumer.
    Custom { kind: StrCow, id: StrCow },
}

impl Resource {
    /// Creates a new [`Resource::Custom`] with the given kind and id.
    /// You can pass `&'static str` as kind, id, or both in order to avoid allocating memory.
    /// Strings are also accepted and will be moved into the ResourceId.
    pub fn custom(kind: impl Into<StrCow>, id: impl Into<StrCow>) -> Resource {
        Resource::Custom {
            kind: kind.into(),
            id: id.into(),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Resource::LocalMachine => "local_machine",
            Resource::CpuPackage { .. } => "cpu_package",
            Resource::CpuCore { .. } => "cpu_core",
            Resource::Dram { .. } => "dram",
            Resource::Gpu { .. } => "gpu",
            Resource::Custom { kind, id: _ } => kind,
        }
    }

    pub fn id_string(&self) -> Option<String> {
        match self {
            Resource::LocalMachine => None,
            r => Some(r.id_display().to_string()),
        }
    }

    pub fn id_display(&self) -> impl fmt::Display + '_ {
        match self {
            Resource::LocalMachine => LazyDisplayable::Str(""),
            Resource::CpuPackage { id } => LazyDisplayable::U32(*id),
            Resource::CpuCore { id } => LazyDisplayable::U32(*id),
            Resource::Dram { pkg_id } => LazyDisplayable::U32(*pkg_id),
            Resource::Gpu { bus_id } => LazyDisplayable::Str(bus_id),
            Resource::Custom { kind: _, id } => LazyDisplayable::Str(id),
        }
    }

    pub fn parse(kind: impl Into<StrCow>, id: impl Into<StrCow>) -> Result<Self, InvalidResourceError> {
        Resource::custom(kind, id).normalize()
    }

    pub fn normalize(self) -> Result<Self, InvalidResourceError> {
        match self {
            Resource::Custom { kind, id } => match kind.as_ref() {
                "local_machine" => {
                    if id.is_empty() {
                        Ok(Resource::LocalMachine)
                    } else {
                        Err(InvalidResourceError::InvalidId(kind))
                    }
                }
                "cpu_package" => {
                    let id = id.parse().map_err(|_| InvalidResourceError::InvalidId(kind))?;
                    Ok(Resource::CpuPackage { id })
                }
                "cpu_core" => {
                    let id = id.parse().map_err(|_| InvalidResourceError::InvalidId(kind))?;
                    Ok(Resource::CpuCore { id })
                }
                "dram" => {
                    let pkg_id = id.parse().map_err(|_| InvalidResourceError::InvalidId(kind))?;
                    Ok(Resource::Dram { pkg_id })
                }
                "gpu" => Ok(Resource::Gpu { bus_id: id }),
                _ => Ok(Resource::Custom { kind, id }),
            },
            r => Ok(r),
        }
    }
}

impl ResourceConsumer {
    pub fn custom(kind: impl Into<StrCow>, id: impl Into<StrCow>) -> ResourceConsumer {
        ResourceConsumer::Custom {
            kind: kind.into(),
            id: id.into(),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            ResourceConsumer::LocalMachine => "local_machine",
            ResourceConsumer::Process { .. } => "process",
            ResourceConsumer::ControlGroup { .. } => "cgroup",
            ResourceConsumer::Custom { kind, id: _ } => kind,
        }
    }

    pub fn id_string(&self) -> Option<String> {
        match self {
            ResourceConsumer::LocalMachine => None,
            c => Some(c.id_display().to_string()),
        }
    }

    pub fn id_display(&self) -> impl fmt::Display + '_ {
        match self {
            ResourceConsumer::LocalMachine => LazyDisplayable::Str(""),
            ResourceConsumer::Process { pid } => LazyDisplayable::U32(*pid),
            ResourceConsumer::ControlGroup { path } => LazyDisplayable::Str(path),
            ResourceConsumer::Custom { kind: _, id } => LazyDisplayable::Str(id),
        }
    }

    pub fn parse(kind: impl Into<StrCow>, id: impl Into<StrCow>) -> Result<Self, InvalidConsumerError> {
        ResourceConsumer::custom(kind, id).normalize()
    }

    pub fn normalize(self) -> Result<Self, InvalidConsumerError> {
        match self {
            ResourceConsumer::Custom { kind, id } => match kind.as_ref() {
                "process" => {
                    let pid = id.parse().map_err(|_| InvalidConsumerError::InvalidId(kind))?;
                    Ok(ResourceConsumer::Process { pid })
                }
                "cgroup" => Ok(ResourceConsumer::ControlGroup { path: id }),
                _ => Ok(ResourceConsumer::Custom { kind, id }),
            },
            r => Ok(r),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidResourceError {
    #[error("invalid resource identifier for kind {0}")]
    InvalidId(StrCow),
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidConsumerError {
    #[error("invalid consumer identifier for kind {0}")]
    InvalidId(StrCow),
}

enum LazyDisplayable<'a> {
    U32(u32),
    Str(&'a str),
}

impl<'a> fmt::Display for LazyDisplayable<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyDisplayable::U32(id) => write!(f, "{id}"),
            LazyDisplayable::Str(id) => write!(f, "{id}"),
        }
    }
}
