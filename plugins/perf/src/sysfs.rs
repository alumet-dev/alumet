//! sysfs topology: online cpus and their package, and the named PMUs (numeric `type`, `cpumask` /
//! `cpus`, the task-attachable core PMUs).
//!
//! A named PMU (`cpu_core`, `uncore_imc_0`, `power`, …) is addressed by the numeric id in
//! `/sys/bus/event_source/devices/<pmu>/type`, which goes into `perf_event_attr.type`. Its `cpumask`
//! (when present) tells the system-wide PMUs apart from the task-attachable core ones, which expose
//! `cpus` instead.

use std::{fs, io, num::ParseIntError};

use anyhow::Context;

pub(crate) fn parse_cpu_list(cpulist: &str) -> anyhow::Result<Vec<u32>> {
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

/// Read the physical package id a CPU belongs to (`physical_package_id` from sysfs topology).
///
/// Used only as a per-package-consistent identifier to group CPUs by package and to
/// label the CpuPackage/CpuCore/Dram/... resource.
pub(crate) fn package_of(cpu: u32) -> anyhow::Result<u32> {
    let path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/physical_package_id");
    let raw = fs::read_to_string(&path).with_context(|| format!("cannot read {path}"))?;
    raw.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid package id in {path}: {:?}", raw.trim()))
}

/// The online cpus.
pub(crate) fn online_cpus() -> anyhow::Result<Vec<u32>> {
    let path = "/sys/devices/system/cpu/online";
    let list = fs::read_to_string(path).with_context(|| format!("Failed to parse {path}"))?;
    parse_cpu_list(&list)
}

// TODO: it should be possible to handle the enabling/disabling of CPUs, instead
// of assuming that the online CPUs never change.

/// Read a PMU's numeric perf `type` from sysfs.
pub(crate) fn read_pmu_type(pmu: &str) -> anyhow::Result<u32> {
    let path = format!("/sys/bus/event_source/devices/{pmu}/type");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("cannot read {path}; is '{pmu}' a valid PMU? (see /sys/bus/event_source/devices)"))?;
    raw.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid PMU type in {path}: {:?}", raw.trim()))
}

/// Read a PMU's `cpumask`.
pub(crate) fn read_pmu_cpumask(pmu: &str) -> anyhow::Result<Option<Vec<u32>>> {
    read_cpu_list_file(pmu, "cpumask")
}

/// Read a PMU's `cpus`.
pub(crate) fn read_pmu_cpus(pmu: &str) -> anyhow::Result<Option<Vec<u32>>> {
    read_cpu_list_file(pmu, "cpus")
}

fn read_cpu_list_file(pmu: &str, file: &str) -> anyhow::Result<Option<Vec<u32>>> {
    let path = format!("/sys/bus/event_source/devices/{pmu}/{file}");
    match fs::read_to_string(&path) {
        Ok(list) => Ok(Some(
            parse_cpu_list(&list).with_context(|| format!("invalid {file} in {path}"))?,
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {path}")),
    }
}

/// A task-attachable core PMU
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CorePmu {
    pub name: String,
    pub type_: u32,
    pub cpus: Vec<u32>,
    pub package: Option<u32>,
}

/// Enumerate the task-attachable core PMUs
pub(crate) fn core_pmus() -> anyhow::Result<Vec<CorePmu>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/bus/event_source/devices") {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // A core PMU has a `cpus` list and no `cpumask` (that would make it system-wide).
        let Ok(None) = read_pmu_cpumask(&name) else { continue };
        let Ok(Some(cpus)) = read_pmu_cpus(&name) else { continue };
        let Ok(type_) = read_pmu_type(&name) else { continue };
        let package = single_package(&cpus);
        out.push(CorePmu {
            name,
            type_,
            cpus,
            package,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The package shared by every CPU in `cpus`, or `None` if they span several packages
pub(crate) fn single_package(cpus: &[u32]) -> Option<u32> {
    let mut packages = cpus.iter().map(|&c| package_of(c).ok());
    let first = packages.next().flatten()?;
    packages.all(|p| p == Some(first)).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_type_matches_sysfs_for_an_available_pmu() {
        // Reads a real PMU from sysfs; skips cleanly where /sys is unavailable (e.g. a sandbox).
        let Some((pmu, expected)) = first_available_pmu() else {
            eprintln!("skipping read_type_matches_sysfs_for_an_available_pmu: no PMU found in sysfs");
            return;
        };
        assert_eq!(read_pmu_type(&pmu).unwrap(), expected);
    }

    #[test]
    fn read_type_rejects_unknown_pmu() {
        assert!(read_pmu_type("definitely_not_a_pmu_xyz").is_err());
    }

    #[test]
    fn cpumask_absent_reads_as_none() {
        // A PMU with no `cpumask` file (here: a non-existent one) is treated as task-attachable, so
        // the classification is `None` rather than an error.
        assert_eq!(read_pmu_cpumask("definitely_not_a_pmu_xyz").unwrap(), None);
    }

    #[test]
    fn cpumask_present_reads_as_some() {
        // Finds a real system-wide PMU (uncore, power, cstate…) and checks its cpumask is non-empty.
        // Skips cleanly if /sys has none (e.g. a sandbox, or a machine with no such PMU).
        let Some(pmu) = first_pmu_with_cpumask() else {
            eprintln!("skipping cpumask_present_reads_as_some: no system-wide PMU found in sysfs");
            return;
        };
        let cpus = read_pmu_cpumask(&pmu).unwrap().expect("cpumask file exists");
        assert!(!cpus.is_empty(), "cpumask of {pmu} should list at least one CPU");
    }

    /// Find any PMU exposing a `cpumask` in sysfs (a system-wide PMU), returning its name.
    fn first_pmu_with_cpumask() -> Option<String> {
        let entries = std::fs::read_dir("/sys/bus/event_source/devices").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(read_pmu_cpumask(&name), Ok(Some(_))) {
                return Some(name);
            }
        }
        None
    }

    /// Find any PMU exposing a numeric `type` in sysfs, returning its name and type.
    fn first_available_pmu() -> Option<(String, u32)> {
        let entries = std::fs::read_dir("/sys/bus/event_source/devices").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(t) = read_pmu_type(&name) {
                return Some((name, t));
            }
        }
        None
    }
}
