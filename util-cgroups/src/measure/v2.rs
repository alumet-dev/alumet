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
    use std::{fs::File, io::Write};

    use tempfile::tempdir;

    use crate::{
        measure::v2::{cpu::CpuStatCollectorSettings, memory::MemoryStatCollectorSettings, V2Collector},
        Cgroup, CgroupHierarchy, CgroupVersion,
    };

    #[test]
    pub fn test_new_and_measure() -> anyhow::Result<()> {
        let root = tempdir().expect("Failed to create a temporary directory");
        // file 1
        let file_path = root.path().join("cpu.stat");
        let data_cpu = "usage_usec 579\n\
                user_usec 123\n\
                system_usec 456\n\
                nr_periods 1\n\
                nr_throttled 2\n\
                throttled_usec 3\n\
                nr_bursts 4\n\
                burst_usec 5\n";
        let mut file1 = File::create(file_path)?;
        file1.write_all(data_cpu.as_bytes())?;
        // file 2
        let file_path = root.path().join("memory.stat");
        let data_mem = "anon 321\n\
                file 654\n\
                kernel_stack 987\n\
                pagetables 741\n";
        let mut file2 = File::create(file_path)?;
        file2.write_all(data_mem.as_bytes())?;
        // file 3
        let file_path = root.path().join("memory.current");
        let data_mem_cur = "852";
        let mut file3 = File::create(file_path)?;
        file3.write_all(data_mem_cur.as_bytes())?;

        let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
        let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

        let mut io_buf = Vec::new();
        let mut collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;
        let v2stat_res = collector.measure(&mut io_buf);
        assert!(v2stat_res.is_ok());
        let v2stat = v2stat_res.unwrap();
        assert!(v2stat.cpu_stat.is_some());
        assert!(v2stat.memory_stat.is_some());
        assert!(v2stat.memory_current.is_some());
        let cpu_stat = v2stat.cpu_stat.unwrap();
        let mem_stat = v2stat.memory_stat.unwrap();
        let mem_cur = v2stat.memory_current.unwrap();

        assert_eq!(cpu_stat.system.unwrap_or(0), 456);
        assert_eq!(cpu_stat.user.unwrap_or(0), 123);
        assert_eq!(cpu_stat.usage.unwrap_or(0), 579);

        assert_eq!(mem_stat.anon.unwrap_or(0), 321);
        assert_eq!(mem_stat.file.unwrap_or(0), 654);
        assert_eq!(mem_stat.kernel_stack.unwrap_or(0), 987);
        assert_eq!(mem_stat.page_tables.unwrap_or(0), 741);

        assert_eq!(mem_cur, 852);

        Ok(())
    }

    #[test]
    pub fn test_new_and_measure_bad_file_content_key() -> anyhow::Result<()> {
        let root = tempdir().expect("Failed to create a temporary directory");
        // file 1
        let file_path = root.path().join("cpu.stat");
        let data_cpu = "usage 579\n\
                user 123\n\
                system 456\n\
                nr_pe 1\n\
                nr_thrtled 2\n\
                throttd_usec 3\n\
                nr_buts 4\n\
                burstc 5\n";
        let mut file1 = File::create(file_path)?;
        file1.write_all(data_cpu.as_bytes())?;
        // file 2
        let file_path = root.path().join("memory.current");
        let data_mem_cur = "963";
        let mut file3 = File::create(file_path)?;
        file3.write_all(data_mem_cur.as_bytes())?;

        let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
        let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

        let mut io_buf = Vec::new();
        let mut collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;
        let v2stat_res = collector.measure(&mut io_buf);
        assert!(v2stat_res.is_ok());
        let v2stat = v2stat_res.unwrap();
        assert!(v2stat.cpu_stat.is_some());
        assert!(v2stat.memory_stat.is_none());
        assert!(v2stat.memory_current.is_some());
        let cpu_stat = v2stat.cpu_stat.unwrap();
        let mem_cur = v2stat.memory_current.unwrap();

        assert!(cpu_stat.system.is_none());
        assert!(cpu_stat.usage.is_none());
        assert!(cpu_stat.user.is_none());

        assert_eq!(mem_cur, 963);
        Ok(())
    }

    #[test]
    pub fn test_new_files_dont_exist() -> anyhow::Result<()> {
        let root = tempdir().expect("Failed to create a temporary directory");

        let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
        let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

        let mut io_buf = Vec::new();
        let mut collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;

        let v2stat_res = collector.measure(&mut io_buf);

        assert!(v2stat_res.is_ok());
        let v2stat = v2stat_res.unwrap();
        assert!(v2stat.cpu_stat.is_none());
        assert!(v2stat.memory_stat.is_none());
        assert!(v2stat.memory_current.is_none());
        Ok(())
    }

    #[test]
    pub fn test_new_missing_lines() -> anyhow::Result<()> {
        let root = tempdir().expect("Failed to create a temporary directory");
        // file 1
        let file_path = root.path().join("cpu.stat");
        let data_cpu = "usage_usec 579\n\
                burst_usec 5\n";
        let mut file1 = File::create(file_path)?;
        file1.write_all(data_cpu.as_bytes())?;
        // file 2
        let file_path = root.path().join("memory.stat");
        let data_mem = "kernel_stack 987\n\
                pagetables 741\n";
        let mut file2 = File::create(file_path)?;
        file2.write_all(data_mem.as_bytes())?;
        // file 3
        let file_path = root.path().join("memory.current");
        let data_mem_cur = "159";
        let mut file3 = File::create(file_path)?;
        file3.write_all(data_mem_cur.as_bytes())?;

        let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
        let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

        let mut io_buf = Vec::new();
        let mut collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;
        let v2stat_res = collector.measure(&mut io_buf);
        assert!(v2stat_res.is_ok());
        let v2stat = v2stat_res.unwrap();
        assert!(v2stat.cpu_stat.is_some());
        assert!(v2stat.memory_stat.is_some());
        assert!(v2stat.memory_current.is_some());
        let cpu_stat = v2stat.cpu_stat.unwrap();
        let mem_stat = v2stat.memory_stat.unwrap();
        let mem_cur = v2stat.memory_current.unwrap();

        assert!(cpu_stat.system.is_none());
        assert!(cpu_stat.user.is_none());
        assert_eq!(cpu_stat.usage.unwrap_or(0), 579);

        assert!(mem_stat.anon.is_none());
        assert!(mem_stat.file.is_none());
        assert_eq!(mem_stat.kernel_stack.unwrap_or(0), 987);
        assert_eq!(mem_stat.page_tables.unwrap_or(0), 741);

        assert_eq!(mem_cur, 159);

        Ok(())
    }

    #[test]
    pub fn test_new_and_measure_bad_file_content_value() -> anyhow::Result<()> {
        let root = tempdir().expect("Failed to create a temporary directory");
        // file 1
        let file_path = root.path().join("cpu.stat");
        let data_cpu = "usage_usec azer\n\
                user_usec tyui\n\
                system_usec opfg\n\
                nr_periods 1\n\
                nr_throttled 2\n\
                throttled_usec 3\n\
                nr_bursts 4\n\
                burst_usec 5\n";
        let mut file1 = File::create(file_path)?;
        file1.write_all(data_cpu.as_bytes())?;
        // file 2
        let file_path = root.path().join("memory.stat");
        let data_mem = "anon ghj\n\
                file hh\n\
                kernel_stack rez\n\
                pagetables 741\n";
        let mut file2 = File::create(file_path)?;
        file2.write_all(data_mem.as_bytes())?;
        // file 3
        let file_path = root.path().join("memory.current");
        let data_mem_cur = "fghj";
        let mut file3 = File::create(file_path)?;
        file3.write_all(data_mem_cur.as_bytes())?;

        let hierarchy = CgroupHierarchy::manually_unchecked(root.path(), CgroupVersion::V2, vec!["cpu", "memory"]);
        let cgroup = Cgroup::from_fs_path(&hierarchy, root.path().to_path_buf());

        let mut io_buf = Vec::new();
        let mut collector_res = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        );

        assert!(collector_res.is_err());
        Ok(())
    }
}
