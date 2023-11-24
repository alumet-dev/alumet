use std::future::Future;
use std::collections::HashMap;
use crate::AttributeValue;

#[non_exhaustive]
pub enum ResourceId {
    Process { pid: u32 },
    ControlGroup { path: String },
    CpuPackage { id: u32 },
    CpuCore { id: u32 },
    Gpu { pci_bus_id: String }
}

pub trait ResourceExplorer {
    fn attributes_all(&self, resource: ResourceId) -> Box<dyn Future<Output = HashMap<String, AttributeValue>> + Send>;
}
