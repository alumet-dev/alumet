use std::fs::File;
use std::io::{BufRead, BufReader};
use std::num::ParseIntError;
use std::path::Path;

use anyhow::{Context, bail};

/// Parses an ARM cpuinfo value, which may have a hexadecimal prefix.
fn parse_int_radix(s: &str) -> Result<u64, ParseIntError> {
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        u64::from_str_radix(&s[2..], 8)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        u64::from_str_radix(&s[2..], 2)
    } else {
        s.parse()
    }
}

/// Minimal parser for the fields of /proc/cpuinfo needed to resolve a
/// likwid_metrics.json architecture record (family, model, stepping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    pub family: u64,
    pub model: u64,
    pub stepping: Option<u64>,
}

#[derive(Default)]
struct CpuInfoBlock {
    family: Option<u64>,
    model: Option<u64>,
    stepping: Option<u64>,
    arm_family: Option<u64>,
    arm_model: Option<u64>,
    vendor: Option<String>,
}

impl CpuInfo {
    /// Reads CPU information from a file.
    /// Returns an error if processors report different models, steppings, or vendors.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The file doesn't contain required cpu family/model fields
    /// - Values cannot be parsed as non-negative 64-bit integers
    pub fn read<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let file = File::open(&path).with_context(|| format!("Could not open {}", path.as_ref().display()))?;
        let reader = BufReader::new(file);
        let mut block = CpuInfoBlock::default();
        let mut processors = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                if let Some(processor) = block.finish() {
                    processors.push(processor);
                }
                block = CpuInfoBlock::default();
                continue;
            }
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim();
                let value = line[colon_pos + 1..].trim();
                match key {
                    "cpu family" => {
                        block.family = Some(value.parse().with_context(|| format!("invalid {key}: {value}"))?);
                    }
                    "model" => {
                        block.model = Some(value.parse().with_context(|| format!("invalid {key}: {value}"))?);
                    }
                    "stepping" => {
                        block.stepping = Some(value.parse().with_context(|| format!("invalid {key}: {value}"))?);
                    }
                    "vendor_id" => {
                        block.vendor = Some(value.to_owned());
                    }
                    // x86 fields are decimal; ARM fields can use hexadecimal prefixes.
                    "CPU architecture" => {
                        block.arm_family =
                            Some(parse_int_radix(value).with_context(|| format!("invalid {key}: {value}"))?);
                    }
                    "CPU part" => {
                        block.arm_model =
                            Some(parse_int_radix(value).with_context(|| format!("invalid {key}: {value}"))?);
                    }
                    "CPU implementer" => {
                        block.vendor = Some(value.to_owned());
                    }
                    _ => {}
                }
            }
        }
        if let Some(processor) = block.finish() {
            processors.push(processor);
        }

        let Some((info, vendor)) = processors.first() else {
            bail!("Could not find 'cpu family'/'model' in {}", path.as_ref().display());
        };
        if processors
            .iter()
            .any(|(other, other_vendor)| other != info || other_vendor != vendor)
        {
            bail!(
                "heterogeneous CPU models, steppings, or vendors are not supported in {}",
                path.as_ref().display()
            );
        }
        Ok(info.clone())
    }
}

impl CpuInfoBlock {
    fn finish(self) -> Option<(CpuInfo, Option<String>)> {
        let info = CpuInfo {
            family: self.family.or(self.arm_family)?,
            model: self.model.or(self.arm_model)?,
            stepping: self.stepping,
        };
        Some((info, self.vendor))
    }
}

/// Reads CPU information from the default /proc/cpuinfo file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or required fields are missing.
pub fn read_cpu_info() -> anyhow::Result<CpuInfo> {
    CpuInfo::read("/proc/cpuinfo")
}
