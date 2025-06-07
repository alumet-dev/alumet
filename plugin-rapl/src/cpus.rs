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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuVendor {
    Intel,
    Amd,
}

/// Retrieves the CPUs to monitor (one per socket) in order
/// to get RAPL perf counters.
pub fn cpus_to_monitor_with_perf() -> anyhow::Result<Vec<CpuId>> {
    let path = "/sys/devices/power/cpumask";
    let mask = fs::read_to_string(path).with_context(|| format!("failed to read {path}"))?;
    let cpus_and_sockets = parse_cpu_and_socket_list(&mask).with_context(|| format!("failed to parse {path}"))?;
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

        match *bounds.as_slice() {
            [start, end] => Ok((start..=end).collect()),
            [n] => Ok(vec![n]),
            _ => Err(anyhow::anyhow!("invalid cpu_list: {}", item)),
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
    let path = "/sys/devices/system/cpu/online";
    let list = std::fs::read_to_string(path).with_context(|| format!("failed to parse {path}"))?;
    parse_cpu_list(&list)
}

fn run_lscpu() -> anyhow::Result<String> {
    // run: LC_ALL=C lscpu
    let child = Command::new("lscpu")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .spawn()
        .context("lscpu should be executable")?;
    let finished = child.wait_with_output()?;
    Ok(std::str::from_utf8(&finished.stdout)?.to_string())
}

fn parse_cpu_vendor_from_lscpu(lscpu: &str) -> anyhow::Result<CpuVendor> {
    // find the Vendor ID
    let vendor_regex = regex::Regex::new(r"Vendor ID:\s+(\w+)")?;
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

    #[test]
    fn test_parse_cpu_vendor_from_lscpu() -> anyhow::Result<()> {
        let lscpu_result = "Architecture:             x86_64
  CPU op-mode(s):         32-bit, 64-bit
  Address sizes:          46 bits physical, 48 bits virtual
  Byte Order:             Little Endian
CPU(s):                   14
  On-line CPU(s) list:    0-13
Vendor ID:                GenuineIntel
  Model name:             Intel(R) Core(TM) Ultra 5 135U
    CPU family:           6
    Model:                170
    Thread(s) per core:   2
    Core(s) per socket:   12
    Socket(s):            1
    Stepping:             4
    CPU(s) scaling MHz:   40%
    CPU max MHz:          4400.0000
    CPU min MHz:          400.0000
    BogoMIPS:             5376.00
    Flags:                fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush dts acpi mmx fxsr sse sse2 ss ht tm
                          pbe syscall nx pdpe1gb rdtscp lm constant_tsc art arch_perfmon pebs bts rep_good nopl xtopology nonstop_tsc cpuid ap
                          erfmperf tsc_known_freq pni pclmulqdq dtes64 monitor ds_cpl vmx smx est tm2 ssse3 sdbg fma cx16 xtpr pdcm pcid sse4_
                          1 sse4_2 x2apic movbe popcnt tsc_deadline_timer aes xsave avx f16c rdrand lahf_lm abm 3dnowprefetch cpuid_fault epb
                          intel_ppin ssbd ibrs ibpb stibp ibrs_enhanced tpr_shadow flexpriority ept vpid ept_ad fsgsbase tsc_adjust bmi1 avx2
                          smep bmi2 erms invpcid rdseed adx smap clflushopt clwb intel_pt sha_ni xsaveopt xsavec xgetbv1 xsaves split_lock_det
                          ect user_shstk avx_vnni dtherm ida arat pln pts hwp hwp_notify hwp_act_window hwp_epp hwp_pkg_req hfi vnmi umip pku
                          ospke waitpkg gfni vaes vpclmulqdq tme rdpid bus_lock_detect movdiri movdir64b fsrm md_clear serialize pconfig arch_
                          lbr ibt flush_l1d arch_capabilities
Virtualization features:
  Virtualization:         VT-x
Caches (sum of all):
  L1d:                    352 KiB (10 instances)
  L1i:                    640 KiB (10 instances)
  L2:                     10 MiB (5 instances)
  L3:                     12 MiB (1 instance)
NUMA:
  NUMA node(s):           1
  NUMA node0 CPU(s):      0-13
Vulnerabilities:
  Gather data sampling:   Not affected
  Itlb multihit:          Not affected
  L1tf:                   Not affected
  Mds:                    Not affected
  Meltdown:               Not affected
  Mmio stale data:        Not affected
  Reg file data sampling: Not affected
  Retbleed:               Not affected
  Spec rstack overflow:   Not affected
  Spec store bypass:      Mitigation; Speculative Store Bypass disabled via prctl
  Spectre v1:             Mitigation; usercopy/swapgs barriers and __user pointer sanitization
  Spectre v2:             Mitigation; Enhanced / Automatic IBRS; IBPB conditional; RSB filling; PBRSB-eIBRS Not affected; BHI BHI_DIS_S
  Srbds:                  Not affected
  Tsx async abort:        Not affected";
        let cpu_vendor = parse_cpu_vendor_from_lscpu(lscpu_result)?;
        assert_eq!(cpu_vendor, CpuVendor::Intel);
        Ok(())
    }

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
