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
    use std::io::{self, ErrorKind};

    use anyhow::Context;

    use crate::{
        measure::v2::{
            cpu::{self, CpuStatCollectorSettings},
            memory::{self, MemoryStatCollectorSettings},
        },
        Cgroup,
    };

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
        /// Creates a new `V2Collector` for the given cgroup.
        ///
        /// # Available metrics
        ///
        /// The metrics that will be measured depends on:
        /// - the cgroup controllers that are enabled
        /// - the configuration of the Linux kernel
        /// - the collectors' settings passed to this method
        pub fn new(
            cgroup: Cgroup<'_>,
            memory_stat_settings: MemoryStatCollectorSettings,
            cpu_stat_settings: CpuStatCollectorSettings,
            io_buf: &mut Vec<u8>,
        ) -> anyhow::Result<Self> {
            let cgroup_path = cgroup.fs_path();
            let memory_current_file = cgroup_path.join("memory.current");
            let memory_stat_file = cgroup_path.join("memory.stat");
            let cpu_stat_file = cgroup_path.join("cpu.stat");

            let prepare_memory_current = || -> anyhow::Result<Option<MemoryCurrentCollector>> {
                match MemoryCurrentCollector::new(&memory_current_file) {
                    Ok(res) => Ok(Some(res)),
                    Err(e) if e.kind() == ErrorKind::NotFound => {
                        // the file does not exist, ignore
                        log::warn!(
                            "{} does not exist, some metrics will not be available",
                            memory_current_file.display()
                        );
                        Ok(None)
                    }
                    Err(e) => Err(e.into()),
                }
            };

            let prepare_memory_stat = |io_buf: &mut Vec<u8>| -> anyhow::Result<Option<MemoryStatCollector>> {
                match MemoryStatCollector::new(&memory_stat_file, memory_stat_settings, io_buf) {
                    Ok(res) => Ok(Some(res)),
                    Err(memory::CollectorCreationError::Io(e, _)) if e.kind() == ErrorKind::NotFound => {
                        // the file does not exist, ignore
                        log::warn!(
                            "{} does not exist, some metrics will not be available",
                            memory_stat_file.display()
                        );
                        Ok(None)
                    }
                    Err(e) => Err(e.into()),
                }
            };

            let prepare_cpu_stat = |io_buf: &mut Vec<u8>| -> anyhow::Result<Option<CpuStatCollector>> {
                match CpuStatCollector::new(&cpu_stat_file, cpu_stat_settings, io_buf) {
                    Ok(res) => Ok(Some(res)),
                    Err(cpu::CollectorCreationError::Io(e, _)) if e.kind() == ErrorKind::NotFound => {
                        // the file does not exist, ignore
                        log::warn!(
                            "{} does not exist, some metrics will not be available",
                            cpu_stat_file.display()
                        );
                        Ok(None)
                    }
                    Err(e) => Err(e.into()),
                }
            };

            let error_msg = || format!("collector creation failed for cgroup {}", cgroup.unique_name());

            Ok(Self {
                memory_current: prepare_memory_current().with_context(error_msg)?,
                memory_stat: prepare_memory_stat(io_buf).with_context(error_msg)?,
                cpu_stat: prepare_cpu_stat(io_buf).with_context(error_msg)?,
            })
        }

        /// Collects measurements from the underlying files, using `io_buf` as an intermediary I/O buffer.
        pub fn measure(&mut self, io_buf: &mut Vec<u8>) -> io::Result<V2Stats> {
            // TODO take &mut V2Stats as a parameter to reduce allocations? Profile.

            let memory_current = self.memory_current.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let memory_stat = self.memory_stat.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let cpu_stat = self.cpu_stat.as_mut().map(|c| c.measure(io_buf)).transpose()?;

            Ok(V2Stats {
                memory_current,
                memory_stat,
                cpu_stat,
            })
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    pub fn test_new() -> anyhow::Result<()> {
        let cgroup = Cgroup {
            
        }
        
        let mut io_buf = Vec::new();
        let collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;


    }
}