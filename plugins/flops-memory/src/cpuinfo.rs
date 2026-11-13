use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Parses an integer with automatic radix detection (hex, decimal, or octal).
/// Supports formats: 0x... (hex), 0o... (octal), 0b... (binary), or decimal.
fn parse_int_radix(s: &str) -> Result<i32, Box<dyn std::error::Error>> {
    let s = s.trim();
    if s.starts_with("0x") || s.starts_with("0X") {
        Ok(i32::from_str_radix(&s[2..], 16)?)
    } else if s.starts_with("0o") || s.starts_with("0O") {
        Ok(i32::from_str_radix(&s[2..], 8)?)
    } else if s.starts_with("0b") || s.starts_with("0B") {
        Ok(i32::from_str_radix(&s[2..], 2)?)
    } else {
        Ok(s.parse::<i32>()?)
    }
}

/// Minimal parser for the fields of /proc/cpuinfo needed to resolve a
/// likwid_metrics.json architecture record (family, model, stepping).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuInfo {
    pub family: i32,
    pub model: i32,
    pub stepping: i32,
}

impl Default for CpuInfo {
    fn default() -> Self {
        Self::new()
    }
}

struct CpuInfoBlock {
    info: CpuInfo,
    arm_family: i32,
    arm_model: i32,
    vendor: Option<String>,
}

impl Default for CpuInfoBlock {
    fn default() -> Self {
        Self {
            info: CpuInfo::default(),
            arm_family: -1,
            arm_model: -1,
            vendor: None,
        }
    }
}

impl CpuInfo {
    /// Creates a new CpuInfo with all fields set to -1 (uninitialized).
    fn new() -> Self {
        CpuInfo {
            family: -1,
            model: -1,
            stepping: -1,
        }
    }

    /// Trims whitespace from a string.
    fn trim(s: &str) -> &str {
        s.trim()
    }

    /// Reads CPU information from a file (defaults to /proc/cpuinfo).
    /// Returns an error if processors report different models, steppings, or vendors.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The file doesn't contain required cpu family/model fields
    /// - Values cannot be parsed as integers
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(&path).map_err(|e| format!("Could not open {}: {}", path.as_ref().display(), e))?;
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
                let key = Self::trim(&line[..colon_pos]);
                let value = Self::trim(&line[colon_pos + 1..]);
                match key {
                    "cpu family" => {
                        block.info.family = value.parse::<i32>()?;
                    }
                    "model" => {
                        block.info.model = value.parse::<i32>()?;
                    }
                    "stepping" => {
                        block.info.stepping = value.parse::<i32>()?;
                    }
                    "vendor_id" => {
                        block.vendor = Some(value.to_owned());
                    }
                    // ARM /proc/cpuinfo uses "CPU architecture" and "CPU part"
                    // instead of "cpu family" and "model"
                    "CPU architecture" => {
                        block.arm_family = parse_int_radix(value)?;
                    }
                    // "CPU part" corresponds to the ARM model field
                    "CPU part" => {
                        block.arm_model = parse_int_radix(value)?;
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
            return Err(format!("Could not find 'cpu family'/'model' in {}", path.as_ref().display()).into());
        };
        if processors
            .iter()
            .any(|(other, other_vendor)| other != info || other_vendor != vendor)
        {
            return Err(format!(
                "heterogeneous CPU models, steppings, or vendors are not supported in {}",
                path.as_ref().display()
            )
            .into());
        }
        Ok(info.clone())
    }
}

impl CpuInfoBlock {
    fn finish(mut self) -> Option<(CpuInfo, Option<String>)> {
        if self.info.family < 0 {
            self.info.family = self.arm_family;
        }
        if self.info.model < 0 {
            self.info.model = self.arm_model;
        }
        (self.info.family >= 0 && self.info.model >= 0).then_some((self.info, self.vendor))
    }
}

/// Reads CPU information from the default /proc/cpuinfo file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or required fields are missing.
pub fn read_cpu_info() -> Result<CpuInfo, Box<dyn std::error::Error>> {
    CpuInfo::read("/proc/cpuinfo")
}
