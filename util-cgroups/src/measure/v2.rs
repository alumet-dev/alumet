//! Measure cgroup v2 things.

/// CPU statistics for cgroup v2.
pub mod cpu;

/// Memory statistics for cgroup v2.
pub mod memory;

/// Small zero-cost wrapper around line index.
mod line_index;

/// Easy settings of the collectors with serde.
mod settings;

/// Private serde utilities.
mod serde_util;

/// Mocks for testing.
#[cfg(feature = "manually")]
pub mod mock;

pub use common::{V2Collector, V2Stats};

mod common {
    use anyhow::Context;

    use crate::Cgroup;

    use super::{
        cpu::{CpuStatCollector, CpuStats},
        memory::{MemoryCurrentCollector, MemoryStatCollector, MemoryStats},
    };

    /// Collects cgroup v2 measurements.
    pub struct V2Collector {
        memory_current: Option<MemoryCurrentCollector>,
        memory_stat: Option<MemoryStatCollector>,
        cpu_stat: Option<CpuStatCollector>,
    }

    pub struct V2Stats {
        pub memory_current: Option<u64>,
        pub memory_stat: Option<MemoryStats>,
        pub cpu_stat: Option<CpuStats>,
    }

    impl V2Collector {
        pub fn new<'h>(cgroup: Cgroup<'h>) -> anyhow::Result<Self> {
            let sysfs = cgroup.fs_path();
            let memory_current_file = sysfs.join("memory.current");
            let memory_stat_file = sysfs.join("memory.stat");
            let cpu_stat_file = sysfs.join("cpu.stat");

            todo!()
        }
    }
}
