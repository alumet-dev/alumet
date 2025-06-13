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

#[derive(Debug, Serialize)]
pub struct CpuStatCollectorSettings {
    pub usage: bool,
    pub user: bool,
    pub system: bool,
}

impl EnabledKeys for CpuStatCollectorSettings {}

impl Default for CpuStatCollectorSettings {
    fn default() -> Self {
        Self {
            usage: true,
            user: true,
            system: true,
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
