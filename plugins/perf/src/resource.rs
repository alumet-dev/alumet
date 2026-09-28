//! The Alumet [`Resource`] measured by a perf counter, one function per way of opening it.
use alumet::resources::Resource;

use crate::event::CoreBinding;
use crate::sysfs;

/// A counter attached to a process, on any cpu: the package of the core PMU it is bound to, or the
/// whole machine when it is unbound or its PMU spans several packages.
pub fn for_process(binding: Option<&CoreBinding>) -> Resource {
    match binding.and_then(|b| b.package) {
        Some(id) => Resource::CpuPackage { id },
        None => Resource::LocalMachine,
    }
}

/// A counter attached to a cgroup, which is always opened on a single cpu.
pub fn for_cgroup_on_cpu(cpu: u32) -> Resource {
    Resource::CpuCore { id: cpu }
}

/// A counter on a system-wide PMU (uncore, `power`, `cstate_*`, …), read from `cpu`.
pub fn for_system_wide_pmu(pmu: &str, cpu: u32) -> Resource {
    let package = sysfs::package_of(cpu).ok();
    match (pmu, package) {
        // per-core PMU: the reader CPU *is* the physical core.
        ("cstate_core", _) => Resource::CpuCore { id: cpu },
        // memory controllers: the RAM of the reader's package.
        (p, Some(pkg)) if p.starts_with("uncore_imc") => Resource::Dram { pkg_id: pkg },
        // package-scoped PMUs we know: RAPL and package C-states.
        (p, Some(pkg)) if p == "power" || p.starts_with("cstate_pkg") => Resource::CpuPackage { id: pkg },
        // anything else (other uncore boxes, unknown PMUs): explicit rather than guessed.
        _ => Resource::Custom {
            kind: pmu.to_owned().into(),
            id: cpu.to_string().into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_wide_pmu_maps_core_and_unknown_pmus() {
        // cstate_core: the reader CPU is the physical core (no topology lookup needed).
        assert_eq!(for_system_wide_pmu("cstate_core", 2), Resource::CpuCore { id: 2 });
        // An unknown PMU: explicit Custom, collision-free (id = the reader CPU).
        assert_eq!(
            for_system_wide_pmu("uncore_cbox_0", 5),
            Resource::Custom {
                kind: "uncore_cbox_0".to_owned().into(),
                id: "5".to_owned().into(),
            }
        );
    }

    #[test]
    fn system_wide_pmu_maps_package_pmus_when_topology_is_available() {
        // power -> package, uncore_imc -> that package's DRAM. Needs sysfs topology; skip if absent.
        let Ok(pkg) = sysfs::package_of(0) else {
            eprintln!("skipping system_wide_pmu_maps_package_pmus_when_topology_is_available: no topology");
            return;
        };
        assert_eq!(for_system_wide_pmu("power", 0), Resource::CpuPackage { id: pkg });
        assert_eq!(for_system_wide_pmu("uncore_imc_0", 0), Resource::Dram { pkg_id: pkg });
    }

    #[test]
    fn process_is_the_package_of_its_core_pmu() {
        let binding = |package| CoreBinding {
            pmu: "cpu_core".to_owned(),
            cpus: vec![0, 1],
            package,
            partial: false,
        };
        assert_eq!(for_process(Some(&binding(Some(1)))), Resource::CpuPackage { id: 1 });
        // A PMU spanning several packages, or no PMU at all: the whole machine.
        assert_eq!(for_process(Some(&binding(None))), Resource::LocalMachine);
        assert_eq!(for_process(None), Resource::LocalMachine);
    }
}
