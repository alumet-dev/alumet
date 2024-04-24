use anyhow::{anyhow, Context};
use std::{
    fs,
    num::ParseIntError,
    process::{Command, Stdio},
};

/// Cpu id and socket (package) id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuId {
    pub cpu: u32,
    pub socket: u32,
}

/// Cpu vendor that supports RAPL energy counters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CpuVendor {
    Intel,
    Amd,
}

/// Retrieves the CPUs to monitor (one per socket) in order
/// to get RAPL perf counters.
pub fn cpus_to_monitor() -> anyhow::Result<Vec<CpuId>> {
    let mask = fs::read_to_string("/sys/devices/power/cpumask")?;
    let cpus_and_sockets = parse_cpu_and_socket_list(&mask)?;
    Ok(cpus_and_sockets)
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

fn parse_cpu_list(cpulist: &str) -> anyhow::Result<Vec<u32>> {
    // handles "n" or "start-end"
    fn parse_cpulist_item(item: &str) -> anyhow::Result<Vec<u32>> {
        let bounds: Vec<u32> = item
            .split('-')
            .map(str::parse)
            .collect::<Result<Vec<u32>, ParseIntError>>()?;

        match bounds.as_slice() {
            &[start, end] => Ok((start..=end).collect()),
            &[n] => Ok(vec![n]),
            _ => Err(anyhow::anyhow!("invalid cpulist: {}", item)),
        }
    }

    // this can be "0,64" or "0-1" or maybe "0-1,64-66"
    let cpus: Vec<u32> = cpulist
        .trim_end()
        .split(',')
        .map(parse_cpulist_item)
        .collect::<anyhow::Result<Vec<Vec<u32>>>>()?
        .into_iter() // not the same as iter() !
        .flatten()
        .collect();

    Ok(cpus)
}

pub fn online_cpus() -> anyhow::Result<Vec<u32>> {
    let list = fs::read_to_string("/sys/devices/system/cpu/online")?;
    parse_cpu_list(&list)
}

pub fn cpu_vendor() -> anyhow::Result<CpuVendor> {
    // run: LC_ALL=C lscpu
    let child = Command::new("lscpu")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .spawn()
        .context("lscpu should be executable")?;
    let finished = child.wait_with_output()?;
    let stdout = std::str::from_utf8(&finished.stdout)?;

    // find the Vendor ID
    let vendor_regex = regex::Regex::new(r"Vendor ID:\s+(\w+)")?;
    let group = vendor_regex
        .captures(stdout)
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

#[cfg(test)]
mod tests {
    use super::{parse_cpu_and_socket_list, CpuId};

    #[test]
    fn test_parse_cpumask() -> anyhow::Result<()> {
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
}
