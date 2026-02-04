use anyhow::{Context, anyhow};
use regex::Regex;
use std::{
    fs::read_to_string,
    num::ParseIntError,
    process::{Command, Stdio},
    str::from_utf8,
};

/// Cpu id and socket (package) id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuId {
    pub cpu: u32,
    pub socket: u32,
}

/// Cpu vendor that supports RAPL energy counters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuVendor {
    Intel,
    Amd,
}

fn parse_cpu_and_socket_list(cpulist: &str) -> anyhow::Result<Vec<CpuId>> {
    let cpus = parse_cpu_list(cpulist);

    // here we assume that /sys/devices/power/cpumask returns one cpu per socket
    let cpus_and_sockets = cpus?
        .into_iter()
        .enumerate()
        .map(|(i, cpu)| CpuId { cpu, socket: i as u32 })
        .collect();

    Ok(cpus_and_sockets)
}

fn cpus_to_monitor_with_perf_path(path: &str) -> anyhow::Result<Vec<CpuId>> {
    let cpulist = read_to_string(path).with_context(|| format!("failed to read {path}"))?;
    parse_cpu_and_socket_list(&cpulist).with_context(|| format!("failed to parse {path}"))
}

/// Retrieves the CPUs to monitor (one per socket) in order
/// to get RAPL perf counters
pub fn cpus_to_monitor_with_perf() -> anyhow::Result<Vec<CpuId>> {
    cpus_to_monitor_with_perf_path("/sys/devices/power/cpumask")
}

fn parse_cpu_list(cpulist: &str) -> anyhow::Result<Vec<u32>> {
    // handles "n" or "start-end"
    fn parse_cpulist_item(item: &str) -> anyhow::Result<Vec<u32>> {
        let bounds = item
            .split('-')
            .map(str::parse)
            .collect::<Result<Vec<u32>, ParseIntError>>()?;

        match *bounds.as_slice() {
            [start, end] => Ok((start..=end).collect()),
            [n] => Ok(vec![n]),
            _ => Err(anyhow::anyhow!("invalid cpu_list: {item}")),
        }
    }

    // this can be "0,64" or "0-1" or maybe "0-1,64-66"
    let cpus = cpulist
        .trim_end()
        .split(',')
        .map(parse_cpulist_item)
        .collect::<anyhow::Result<Vec<Vec<u32>>>>()?
        .into_iter() // not the same as iter() !
        .flatten()
        .collect();

    Ok(cpus)
}

fn online_cpus_path(path: &str) -> anyhow::Result<Vec<u32>> {
    let list = read_to_string(path).with_context(|| format!("failed to parse {path}"))?;
    parse_cpu_list(&list)
}

pub fn online_cpus() -> anyhow::Result<Vec<u32>> {
    online_cpus_path("/sys/devices/system/cpu/online")
}

fn run_lscpu() -> anyhow::Result<String> {
    // run: LC_ALL=C lscpu
    let child = Command::new("lscpu")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .spawn()
        .context("lscpu should be executable")?;
    let finished = child.wait_with_output()?;
    Ok(from_utf8(&finished.stdout)?.to_string())
}

fn parse_cpu_vendor_from_lscpu(lscpu: &str) -> anyhow::Result<CpuVendor> {
    // find the Vendor ID
    let vendor_regex = Regex::new(r"Vendor ID:\s+(\w+)")?;
    let group = vendor_regex
        .captures(lscpu)
        .context("vendor id not found in lscpu output")?
        .get(1)
        .unwrap();
    let vendor = group.as_str().trim();

    // turn it into the right enum variant
    match vendor {
        "AuthenticAMD" => Ok(CpuVendor::Amd),
        "GenuineIntel" => Ok(CpuVendor::Intel),
        _ => Err(anyhow!("Unsupported CPU vendor {vendor}")),
    }
}

pub fn cpu_vendor() -> anyhow::Result<CpuVendor> {
    let lscpu_result = run_lscpu()?;
    parse_cpu_vendor_from_lscpu(&lscpu_result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_cpu_and_socket_list() -> anyhow::Result<()> {
        let single = "0";
        assert_eq!(parse_cpu_and_socket_list(single)?, vec![CpuId { cpu: 0, socket: 0 }]);

        let comma = "0,64";
        assert_eq!(
            parse_cpu_and_socket_list(comma)?,
            vec![CpuId { cpu: 0, socket: 0 }, CpuId { cpu: 64, socket: 1 }]
        );

        let caret = "0-1";
        assert_eq!(
            parse_cpu_and_socket_list(caret)?,
            vec![CpuId { cpu: 0, socket: 0 }, CpuId { cpu: 1, socket: 1 }]
        );

        let combined = "1-3,5-6";
        assert_eq!(
            parse_cpu_and_socket_list(combined)?,
            vec![
                CpuId { cpu: 1, socket: 0 },
                CpuId { cpu: 2, socket: 1 },
                CpuId { cpu: 3, socket: 2 },
                CpuId { cpu: 5, socket: 3 },
                CpuId { cpu: 6, socket: 4 },
            ]
        );

        Ok(())
    }

    #[test]
    fn test_parse_cpulist_invalid_item() {
        let cpulist = "1-2-3";
        let result = parse_cpu_list(cpulist).unwrap_err();
        assert!(result.to_string().contains("invalid cpu_list"));
    }

    #[test]
    fn test_cpus_to_monitor_with_perf_path() -> anyhow::Result<()> {
        let mut file = NamedTempFile::new()?;
        let cpulist = "1-3,5-6";
        writeln!(file, "{cpulist}")?;

        let result = cpus_to_monitor_with_perf_path(file.path().to_str().unwrap())?;
        assert_eq!(
            result,
            vec![
                CpuId { cpu: 1, socket: 0 },
                CpuId { cpu: 2, socket: 1 },
                CpuId { cpu: 3, socket: 2 },
                CpuId { cpu: 5, socket: 3 },
                CpuId { cpu: 6, socket: 4 },
            ]
        );

        Ok(())
    }

    #[test]
    fn test_online_cpus_path() -> anyhow::Result<()> {
        let mut file = NamedTempFile::new()?;
        let cpulist = "1-3,5-6";
        writeln!(file, "{cpulist}")?;

        let result = online_cpus_path(file.path().to_str().unwrap())?;
        assert_eq!(result, vec![1, 2, 3, 5, 6]);

        Ok(())
    }

    #[test]
    fn test_parse_cpu_vendor_from_lscpu() {
        let intel = "Vendor ID:                GenuineIntel";
        let amd = "Vendor ID:                AuthenticAMD";
        assert_eq!(parse_cpu_vendor_from_lscpu(intel).unwrap(), CpuVendor::Intel);
        assert_eq!(parse_cpu_vendor_from_lscpu(amd).unwrap(), CpuVendor::Amd);
    }

    #[test]
    fn test_parse_cpu_vendor_from_lscpu_unsupported() {
        let lscpu = "Vendor ID:                UnknownVendor";
        let result = parse_cpu_vendor_from_lscpu(lscpu).unwrap_err();
        assert!(result.to_string().contains("Unsupported CPU vendor"));
    }

    #[test]
    fn test_parse_cpu_vendor_from_lscpu_missing() {
        let lscpu = "No vendor ID";
        let result = parse_cpu_vendor_from_lscpu(lscpu).unwrap_err();
        assert!(result.to_string().contains("vendor id not found in lscpu output"));
    }
}
