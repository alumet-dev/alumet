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
    /// Returns a CpuInfo struct with family, model, and stepping values.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened
    /// - The file doesn't contain required cpu family/model fields
    /// - Values cannot be parsed as integers
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(&path).map_err(|e| {
            format!(
                "Could not open {}: {}",
                path.as_ref().display(),
                e
            )
        })?;

        let reader = BufReader::new(file);
        let mut info = CpuInfo::new();
        // ARM /proc/cpuinfo has no "cpu family"/"model" fields; use these as a fallback.
        let mut arm_family = -1;
        let mut arm_model = -1;

        for line in reader.lines() {
            let line = line?;

            // Only the first processor block is needed: family/model/stepping are uniform per-host.
            if line.is_empty() && info.family >= 0 && info.model >= 0 {
                break;
            }

            if let Some(colon_pos) = line.find(':') {
                let key = Self::trim(&line[..colon_pos]);
                let value = Self::trim(&line[colon_pos + 1..]);

                match key {
                    "cpu family" => {
                        info.family = value.parse::<i32>()?;
                    }
                    "model" => {
                        info.model = value.parse::<i32>()?;
                    }
                    "stepping" => {
                        info.stepping = value.parse::<i32>()?;
                    }
                    // ARM /proc/cpuinfo uses "CPU architecture" and "CPU part"
                    // instead of "cpu family" and "model"
                    "CPU architecture" => {
                        arm_family = parse_int_radix(value)?;
                    }
                    // "CPU part" corresponds to the ARM model field
                    "CPU part" => {
                        arm_model = parse_int_radix(value)?;
                    }
                    _ => {}
                }
            }
        }

        // Use ARM fallback values if x86 values were not found
        if info.family < 0 && arm_family >= 0 {
            info.family = arm_family;
        }
        if info.model < 0 && arm_model >= 0 {
            info.model = arm_model;
        }

        if info.family < 0 || info.model < 0 {
            return Err(format!(
                "Could not find 'cpu family'/'model' in {}",
                path.as_ref().display()
            )
            .into());
        }

        Ok(info)
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
