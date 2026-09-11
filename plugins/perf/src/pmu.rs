//! sysfs helpers for named PMUs: splitting the `pmu/terms` form, reading a PMU's numeric `type`, its
//! `cpumask` / `cpus`, and enumerating the task-attachable core PMUs.
//!
//! A named PMU (`cpu_core`, `uncore_imc_0`, `power`, …) is addressed by the numeric id in
//! `/sys/bus/event_source/devices/<pmu>/type`, which goes into `perf_event_attr.type`. Its `cpumask`
//! (when present) tells the system-wide PMUs apart from the task-attachable core ones, which expose
//! `cpus` instead.

use std::{fs, io};

use anyhow::Context;

use crate::cpu::parse_cpu_list;

/// Split a `pmu/terms` string into its PMU name and inner term list.
pub fn split(name: &str) -> Option<(&str, &str)> {
    let (pmu, terms) = name.split_once('/')?;
    let terms = terms.strip_suffix('/').unwrap_or(terms);
    if pmu.is_empty() || terms.is_empty() || terms.contains('/') {
        return None;
    }
    Some((pmu, terms))
}

/// Read a PMU's numeric perf `type` from sysfs.
pub fn read_type(pmu: &str) -> anyhow::Result<u32> {
    let path = format!("/sys/bus/event_source/devices/{pmu}/type");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("cannot read {path}; is '{pmu}' a valid PMU? (see /sys/bus/event_source/devices)"))?;
    raw.trim()
        .parse::<u32>()
        .with_context(|| format!("invalid PMU type in {path}: {:?}", raw.trim()))
}

/// Read a PMU's `cpumask`.
pub fn read_cpumask(pmu: &str) -> anyhow::Result<Option<Vec<u32>>> {
    read_cpu_list_file(pmu, "cpumask")
}

/// Read a PMU's `cpus`.
pub fn read_cpus(pmu: &str) -> anyhow::Result<Option<Vec<u32>>> {
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
pub struct CorePmu {
    pub name: String,
    pub type_: u32,
    pub cpus: Vec<u32>,
    pub package: Option<u32>,
}

/// Enumerate the task-attachable core PMUs
pub fn core_pmus() -> anyhow::Result<Vec<CorePmu>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir("/sys/bus/event_source/devices") {
        Ok(e) => e,
        Err(_) => return Ok(out),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // A core PMU has a `cpus` list and no `cpumask` (that would make it system-wide).
        if !matches!(read_cpumask(&name), Ok(None)) {
            continue;
        }
        let Ok(Some(cpus)) = read_cpus(&name) else { continue };
        let Ok(type_) = read_type(&name) else { continue };
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
pub fn single_package(cpus: &[u32]) -> Option<u32> {
    let mut packages = cpus.iter().map(|&c| crate::cpu::package_of(c).ok());
    let first = packages.next().flatten()?;
    packages.all(|p| p == Some(first)).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_extracts_pmu_and_terms() {
        assert_eq!(split("uncore_imc_0/r0x1/"), Some(("uncore_imc_0", "r0x1")));
        assert_eq!(split("cpu_core/INSTRUCTIONS"), Some(("cpu_core", "INSTRUCTIONS"))); // closing / optional
        assert_eq!(
            split("cpu/event=0x2e,umask=0x41/"),
            Some(("cpu", "event=0x2e,umask=0x41"))
        );
    }

    #[test]
    fn split_rejects_other_shapes() {
        assert_eq!(split("r3c"), None); // no `/` at all
        assert_eq!(split("cpu_core/"), None); // empty terms
        assert_eq!(split("/r3c/"), None); // empty PMU
        assert_eq!(split("a/b/c/"), None); // nested slash
    }

    #[test]
    fn read_type_matches_sysfs_for_an_available_pmu() {
        // Reads a real PMU from sysfs; skips cleanly where /sys is unavailable (e.g. a sandbox).
        let Some((pmu, expected)) = first_available_pmu() else {
            eprintln!("skipping read_type_matches_sysfs_for_an_available_pmu: no PMU found in sysfs");
            return;
        };
        assert_eq!(read_type(&pmu).unwrap(), expected);
    }

    #[test]
    fn read_type_rejects_unknown_pmu() {
        assert!(read_type("definitely_not_a_pmu_xyz").is_err());
    }

    #[test]
    fn cpumask_absent_reads_as_none() {
        // A PMU with no `cpumask` file (here: a non-existent one) is treated as task-attachable, so
        // the classification is `None` rather than an error.
        assert_eq!(read_cpumask("definitely_not_a_pmu_xyz").unwrap(), None);
    }

    #[test]
    fn cpumask_present_reads_as_some() {
        // Finds a real system-wide PMU (uncore, power, cstate…) and checks its cpumask is non-empty.
        // Skips cleanly if /sys has none (e.g. a sandbox, or a machine with no such PMU).
        let Some(pmu) = first_pmu_with_cpumask() else {
            eprintln!("skipping cpumask_present_reads_as_some: no system-wide PMU found in sysfs");
            return;
        };
        let cpus = read_cpumask(&pmu).unwrap().expect("cpumask file exists");
        assert!(!cpus.is_empty(), "cpumask of {pmu} should list at least one CPU");
    }

    /// Find any PMU exposing a `cpumask` in sysfs (a system-wide PMU), returning its name.
    fn first_pmu_with_cpumask() -> Option<String> {
        let entries = std::fs::read_dir("/sys/bus/event_source/devices").ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(read_cpumask(&name), Ok(Some(_))) {
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
            if let Ok(t) = read_type(&name) {
                return Some((name, t));
            }
        }
        None
    }
}
