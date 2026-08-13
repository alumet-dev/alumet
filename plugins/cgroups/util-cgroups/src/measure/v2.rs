//! Measure cgroup v2 things.

/// CPU statistics for cgroup v2.
pub mod cpu;

/// Memory statistics for cgroup v2.
pub mod memory;

/// IO statistics for cgroup v2.
pub mod io;

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
        Cgroup,
        measure::v2::{
            cpu::{self, CpuStatCollectorSettings},
            io::{IoPressureCollector, IoPressureCollectorSettings},
            memory::{self, MemoryStatCollectorSettings, MemorySwapCurrentCollector, MemorySwapMaxCollector},
        },
    };

    use super::{
        cpu::{CpuStatCollector, CpuStats},
        io::IoPressureStats,
        memory::{MemoryCurrentCollector, MemoryMaxCollector, MemoryStatCollector, MemoryStats},
    };

    /// Collects cgroup v2 measurements.
    pub struct V2Collector {
        memory_current: Option<MemoryCurrentCollector>,
        memory_max: Option<MemoryMaxCollector>,
        memory_stat: Option<MemoryStatCollector>,
        memory_swap_current: Option<MemorySwapCurrentCollector>,
        memory_swap_max: Option<MemorySwapMaxCollector>,
        cpu_stat: Option<CpuStatCollector>,
        io_pressure: Option<IoPressureCollector>,
    }

    pub struct V2Stats {
        pub memory_current: Option<u64>,
        pub memory_max: Option<u64>,
        pub memory_stat: Option<MemoryStats>,
        pub memory_swap_current: Option<u64>,
        pub memory_swap_max: Option<u64>,
        pub cpu_stat: Option<CpuStats>,
        pub io_pressure: Option<IoPressureStats>,
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
            io_pressure_settings: IoPressureCollectorSettings,
            io_buf: &mut Vec<u8>,
        ) -> anyhow::Result<Self> {
            let cgroup_path = cgroup.fs_path();
            let memory_current_file = cgroup_path.join("memory.current");
            let memory_max_file = cgroup_path.join("memory.max");
            let memory_stat_file = cgroup_path.join("memory.stat");
            let memory_swap_current_file = cgroup_path.join("memory.swap.current");
            let memory_swap_max_file = cgroup_path.join("memory.swap.max");
            let cpu_stat_file = cgroup_path.join("cpu.stat");
            let io_pressure_file = cgroup_path.join("io.pressure");

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

            let prepare_memory_max = || -> anyhow::Result<Option<MemoryMaxCollector>> {
                match MemoryMaxCollector::new(&memory_max_file) {
                    Ok(res) => Ok(Some(res)),
                    Err(e) if e.kind() == ErrorKind::NotFound => {
                        // the file does not exist, ignore
                        log::warn!(
                            "{} does not exist, some metrics will not be available",
                            memory_max_file.display()
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

            let prepare_memory_swap_current = || -> anyhow::Result<Option<MemorySwapCurrentCollector>> {
                match MemorySwapCurrentCollector::new(&memory_swap_current_file) {
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

            let prepare_memory_swap_max = || -> anyhow::Result<Option<MemorySwapMaxCollector>> {
                match MemorySwapMaxCollector::new(&memory_swap_max_file) {
                    Ok(res) => Ok(Some(res)),
                    Err(e) if e.kind() == ErrorKind::NotFound => {
                        // the file does not exist, ignore
                        log::warn!(
                            "{} does not exist, some metrics will not be available",
                            memory_max_file.display()
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

            let prepare_io_pressure = |io_buf: &mut Vec<u8>| -> anyhow::Result<Option<IoPressureCollector>> {
                match IoPressureCollector::new(&io_pressure_file, io_pressure_settings, io_buf) {
                    Ok(res) => Ok(Some(res)),
                    Err(super::io::CollectorCreationError::Io(e, _)) if e.kind() == ErrorKind::NotFound => {
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
                memory_max: prepare_memory_max().with_context(error_msg)?,
                memory_stat: prepare_memory_stat(io_buf).with_context(error_msg)?,
                memory_swap_current: prepare_memory_swap_current().with_context(error_msg)?,
                memory_swap_max: prepare_memory_swap_max().with_context(error_msg)?,
                cpu_stat: prepare_cpu_stat(io_buf).with_context(error_msg)?,
                io_pressure: prepare_io_pressure(io_buf).with_context(error_msg)?,
            })
        }

        /// Collects measurements from the underlying files, using `io_buf` as an intermediary I/O buffer.
        pub fn measure(&mut self, io_buf: &mut Vec<u8>) -> io::Result<V2Stats> {
            // TODO take &mut V2Stats as a parameter to reduce allocations? Profile.

            let memory_current = self.memory_current.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let memory_max = self.memory_max.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let memory_stat = self.memory_stat.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let memory_swap_current = self
                .memory_swap_current
                .as_mut()
                .map(|c| c.measure(io_buf))
                .transpose()?;
            let memory_swap_max = self.memory_swap_max.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let cpu_stat = self.cpu_stat.as_mut().map(|c| c.measure(io_buf)).transpose()?;
            let io_pressure = self.io_pressure.as_mut().map(|c| c.measure(io_buf)).transpose()?;

            Ok(V2Stats {
                memory_current,
                memory_max,
                memory_stat,
                memory_swap_current,
                memory_swap_max,
                cpu_stat,
                io_pressure,
            })
        }
    }
}
