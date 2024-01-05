use std::future::Future;
use std::collections::HashMap;
use crate::metric::AttributeValue;

/// Hardware or software entity for which metrics can be gathered.
#[non_exhaustive]
pub enum ResourceId {
    /// The whole local machine, for instance the whole physical server.
    LocalMachine,
    /// A process at the OS level.
    Process { pid: u32 },
    /// A control group, often abbreviated cgroup.
    ControlGroup { path: String },
    /// A physical CPU package (which is not the same as a NUMA node).
    CpuPackage { id: u32 },
    /// A CPU core.
    CpuCore { id: u32 },
    /// The RAM attached to a CPU package.
    Dram { pkg_id: u32 },
    /// A dedicated GPU.
    Gpu { bus_id: String },
}

pub trait ResourceExplorer {
    fn attributes_all(&self, resource: ResourceId) -> Box<dyn Future<Output = HashMap<String, AttributeValue>> + Send>;
}
