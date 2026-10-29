use std::io::{Read, Seek, SeekFrom};
use std::{fs::File, io, path::Path};

use serde::Serialize;

use super::line_index::LineIndex;
use crate::measure::{
    parse::{SelectiveStatFile, StatFileBuilder},
    v2::settings::EnabledKeys,
};

/// Collects measurements from `cpu.stat`.
pub struct CpuStatCollector {
    stat_file: SelectiveStatFile,
    mapping: CpuStatMapping,
}

#[derive(Default)]
struct CpuStatMapping {
    usage: LineIndex,
    user: LineIndex,
    system: LineIndex,
}

pub struct CpusetCpusEffective {
    cpus: File,
}

#[derive(Debug, Serialize)]
pub struct CpuStatCollectorSettings {
    pub usage_usec: bool,
    pub user_usec: bool,
    pub system_usec: bool,
}

impl EnabledKeys for CpuStatCollectorSettings {}

impl Default for CpuStatCollectorSettings {
    fn default() -> Self {
        Self {
            usage_usec: true,
            user_usec: true,
            system_usec: true,
        }
    }
}

/// Represents the measurements extracted from the `cpu.stat` file.
#[derive(Debug, Default)]
pub struct CpuStats {
    pub usage: Option<u64>,
    pub user: Option<u64>,
    pub system: Option<u64>,
    // could be extended to manage other measurements
}

pub type CollectorCreationError = super::memory::CollectorCreationError;

impl CpuStatCollector {
    pub fn new<P: AsRef<Path>>(
        path: P,
        settings: CpuStatCollectorSettings,
        io_buf: &mut Vec<u8>,
    ) -> Result<Self, CollectorCreationError> {
        let path = path.as_ref();

        let file = File::open(path).map_err(|e| CollectorCreationError::Io(e, path.into()))?;

        let keys = settings.enabled_keys()?;
        let (stat_file, stat_mapping) = StatFileBuilder::new(file, &keys)
            .build(io_buf.as_mut())
            .map_err(|e| CollectorCreationError::Io(e, path.into()))?;

        let mut mapping = CpuStatMapping::default();
        if let Some(i) = stat_mapping.line_index("usage_usec") {
            mapping.usage = i.into();
        }
        if let Some(i) = stat_mapping.line_index("user_usec") {
            mapping.user = i.into();
        }
        if let Some(i) = stat_mapping.line_index("system_usec") {
            mapping.system = i.into();
        }

        if !stat_mapping.keys_not_found().is_empty() {
            log::warn!(
                "keys not found in {}: {}",
                path.display(),
                stat_mapping.keys_not_found().join(", ")
            )
        }

        Ok(Self { stat_file, mapping })
    }

    /// Collects measurements from the underlying "file", using `io_buf` as an intermediary I/O buffer.
    pub fn measure(&mut self, io_buf: &mut Vec<u8>) -> io::Result<CpuStats> {
        let mut res = CpuStats::default();
        unsafe {
            self.stat_file.read(io_buf, |i, _k, v| match i {
                i if i == self.mapping.usage.0 => {
                    res.usage = Some(v);
                }
                i if i == self.mapping.user.0 => {
                    res.user = Some(v);
                }
                i if i == self.mapping.system.0 => {
                    res.system = Some(v);
                }
                _ => (),
            })
        }?;
        Ok(res)
    }
}

fn count_cpus(cpulist: &str) -> anyhow::Result<u32> {
    let mut count = 0;

    for item in cpulist.trim().split(',') {
        let bounds: Vec<u32> = item.split('-').map(str::parse).collect::<Result<Vec<u32>, _>>()?;

        match bounds.as_slice() {
            [start, end] if start <= end => {
                count += end - start + 1;
            }
            [n] => {
                count += 1;
            }
            _ => anyhow::bail!("invalid cpu_list: {}", item),
        }
    }

    Ok(count)
}

pub fn parse_cpuset(file: &mut File) -> anyhow::Result<u32> {
    file.seek(SeekFrom::Start(0))?;

    let mut content = String::new();
    file.read_to_string(&mut content)?;

    count_cpus(content.trim())
}

impl CpusetCpusEffective {
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let cpus = File::open(path)?;
        Ok(Self { cpus })
    }

    /// Collects measurements from the underlying "file",
    pub fn measure(&mut self) -> io::Result<u64> {
        let count = parse_cpuset(&mut self.cpus).map_err(|e| io::Error::other(e))?;
        Ok(u64::from(count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_cpu() {
        assert_eq!(count_cpus("0").unwrap(), 1);
        assert_eq!(count_cpus("5").unwrap(), 1);
        assert_eq!(count_cpus("127").unwrap(), 1);
    }

    #[test]
    fn test_cpu_ranges() {
        assert_eq!(count_cpus("0-7").unwrap(), 8);
        assert_eq!(count_cpus("1-4").unwrap(), 4);
        assert_eq!(count_cpus("5-5").unwrap(), 1);
        assert_eq!(count_cpus("0-0").unwrap(), 1);
        assert_eq!(count_cpus("10-19").unwrap(), 10);
    }

    #[test]
    fn test_multiple_cpus() {
        assert_eq!(count_cpus("1,5,9").unwrap(), 3);
        assert_eq!(count_cpus("1,5,9,12-14").unwrap(), 6);
        assert_eq!(count_cpus("0-3,6-7").unwrap(), 6);
        assert_eq!(count_cpus("0-2,4-6,8-10").unwrap(), 9);
    }

    #[test]
    fn test_cpu_list_with_whitespace_at_end() {
        assert_eq!(count_cpus("0-7\n").unwrap(), 8);
        assert_eq!(count_cpus("1,5,9\n").unwrap(), 3);
        assert_eq!(count_cpus("0-3  ").unwrap(), 4);
    }

    #[test]
    fn test_invalid_input() {
        assert!(count_cpus("abc").is_err());
        assert!(count_cpus("1-x").is_err());
        assert!(count_cpus("1-2-3").is_err());
        assert!(count_cpus("3-1").is_err());
    }

    #[test]
    fn test_empty_input() {
        assert!(count_cpus("").is_err());
        assert!(count_cpus("   ").is_err());
    }

    #[test]
    fn test_invalid_items_in_list() {
        assert!(count_cpus("0-2,abc").is_err());
        assert!(count_cpus("0-2,4-x").is_err());
        assert!(count_cpus("0-2,").is_err());
        assert!(count_cpus(",1,2").is_err());
    }

    #[test]
    fn test_large_cpu_ranges() {
        assert_eq!(count_cpus("0-255").unwrap(), 256);
        assert_eq!(count_cpus("0-1023").unwrap(), 1024);
    }
}
