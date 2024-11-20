//! # cgroupv2 file for k8s and oar3 module
//!
//! This module provides functionality for formatting metrics.
//!
use std::str::FromStr;

use alumet::{
    metrics::{MetricCreationError, TypedMetricId},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

#[derive(Debug, PartialEq, Clone)]
pub struct CgroupV2Metric {
    pub name: String,               // Name of a Kubernetes pod
    pub uid: String,                // Unique identification of a Kubernetes pod
    pub namespace: String,          // Resources isolation of a Kubernetes pod
    pub node: String,               // Kubernetes pod node
    pub time_used_tot: u64,         // Total CPU usage time by the cgroup
    pub time_used_user_mode: u64,   // CPU in user mode usage time by the cgroup
    pub time_used_system_mode: u64, // CPU in system mode usage time by the cgroup
    pub anon_used_mem: u64, // Anonymous used memory, corresponding to running process and various allocated memory
    pub file_mem: u64,      // Files memory, corresponding to open files and descriptors
    pub kernel_mem: u64,    // Memory reserved for kernel operations
    pub pagetables_mem: u64, // Memory used to manage correspondence between virtual and physical addresses
    pub total_mem: u64,     // Total memory used by cgroup
}

impl FromStr for CgroupV2Metric {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cgroup_struc_to_ret = CgroupV2Metric {
            name: "".to_owned(),
            uid: "".to_owned(),
            namespace: "".to_owned(),
            node: "".to_owned(),
            time_used_tot: 0,
            time_used_user_mode: 0,
            time_used_system_mode: 0,
            anon_used_mem: 0,
            file_mem: 0,
            kernel_mem: 0,
            pagetables_mem: 0,
            total_mem: 0,
        };

        for line in s.lines() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    // Total CPU usage time by the cgroup
                    "usage_usec" => {
                        cgroup_struc_to_ret.time_used_tot = parts[1].parse::<u64>()?;
                    }
                    // CPU in user mode usage time by the cgroup
                    "user_usec" => {
                        cgroup_struc_to_ret.time_used_user_mode = parts[1].parse::<u64>()?;
                    }
                    // CPU in system mode usage time by the cgroup
                    "system_usec" => {
                        cgroup_struc_to_ret.time_used_system_mode = parts[1].parse::<u64>()?;
                    }
                    // Anonymous used memory, corresponding to running process and various allocated memory
                    "anon" => {
                        cgroup_struc_to_ret.anon_used_mem = parts[1].parse::<u64>()?;
                    }
                    // Files memory, corresponding to open files and descriptors
                    "file" => {
                        cgroup_struc_to_ret.file_mem = parts[1].parse::<u64>()?;
                    }
                    // Reserved memory for kernel operations
                    "kernel_stack" => {
                        cgroup_struc_to_ret.kernel_mem = parts[1].parse::<u64>()?;
                    }
                    // Used memory to manage correspondence between virtual and physical addresses
                    "pagetables" => {
                        cgroup_struc_to_ret.pagetables_mem = parts[1].parse::<u64>()?;
                    }
                    &_ => continue,
                }
            }
        }
        Ok(cgroup_struc_to_ret)
    }
}

#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,         // Total CPU usage time by the cgroup
    pub time_used_user_mode: TypedMetricId<u64>,   // CPU in user mode usage time by the cgroup
    pub time_used_system_mode: TypedMetricId<u64>, // CPU in system mode usage time by the cgroup
    pub anon_used_mem: TypedMetricId<u64>, // Anonymous used memory, corresponding to running process and various allocated memory
    pub file_mem: TypedMetricId<u64>,      // Files memory, corresponding to open files and descriptors
    pub kernel_mem: TypedMetricId<u64>,    // Memory reserved for kernel operations
    pub pagetables_mem: TypedMetricId<u64>, // Memory used to manage correspondence between virtual and physical addresses
    pub total_mem: TypedMetricId<u64>,      // Total memory used by cgroup
}

impl Metrics {
    /// # Function
    ///
    /// * `new` - Public function provides a information base to create metric before sending CPU and memory data,
    /// with `name`, `unit` and `description` parameters.
    ///
    /// # Arguments
    ///
    /// * `alumet` - A AlumetPluginStart structure passed to plugins for the start-up phase.
    ///
    /// # Returns
    ///
    /// * `Result` - Type representing success `Ok` or failure `Err`.
    /// * `MetricCreationError` - Error which can occur when creating a new metric.
    ///
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let usec: PrefixedUnit = PrefixedUnit::micro(Unit::Second);
        let kb: PrefixedUnit = PrefixedUnit::kilo(Unit::Byte);

        Ok(Self {
            // CPU cgroup data
            time_used_tot: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_total",
                usec.clone(),
                "Total CPU usage time by the cgroup",
            )?,
            time_used_user_mode: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_user",
                usec.clone(),
                "CPU in user mode usage time by the cgroup",
            )?,
            time_used_system_mode: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_system",
                usec.clone(),
                "CPU in system mode usage time by the cgroup",
            )?,

            // Memory cgroup data
            anon_used_mem: alumet.create_metric::<u64>(
                "cgroup_memory_anonymous",
                kb.clone(),
                "Anonymous used memory, corresponding to running process and various allocated memory",
            )?,
            file_mem: alumet.create_metric::<u64>(
                "cgroup_memory_file",
                kb.clone(),
                "Files memory, corresponding to open files and descriptors",
            )?,
            kernel_mem: alumet.create_metric::<u64>(
                "cgroup_memory_kernel_stack",
                kb.clone(),
                "Memory reserved for kernel operations",
            )?,
            pagetables_mem: alumet.create_metric::<u64>(
                "cgroup_memory_pagetables",
                kb.clone(),
                "Memory used to manage correspondence between virtual and physical addresses",
            )?,
            total_mem: alumet.create_metric::<u64>("cgroup_memory_total", kb.clone(), "Total memory used by cgroup")?,
        })
    }
}

/** Unitary test for CPU and Memory cgroup files parser **/
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser() {
        let test_cgroup_cpu: String = format!(
            "
            usage_usec      1111\n
            user_usec     222222222222222222\n
            system_usec 33\n
            nr_periods 0\n
            nr_throttled 0\n
            throttled_usec 0"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&test_cgroup_cpu).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 1111 as u64);
        assert_eq!(result.time_used_user_mode, 222222222222222222 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let test2_cgroup_cpu: String = format!(
            "
            system_usec 33"
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&test2_cgroup_cpu).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 0 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 33 as u64);

        let test_cgroup_memory: String = format!(
            "
            anon 222222222222222222
            file        4728882396
            kernel_stack  3686400
            pagetables 0
            percpu 16317568
            sock 12288
            shmem 233824256
            file_mapped 0
            file_dirty 20480,
            ...."
        );
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&test_cgroup_memory).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.anon_used_mem, 222222222222222222 as u64);
        assert_eq!(result.file_mem, 4728882396 as u64);
        assert_eq!(result.kernel_mem, 3686400 as u64);
        assert_eq!(result.pagetables_mem, 0 as u64);

        let test_cgroup_files: String = format!("");
        let result: CgroupV2Metric = CgroupV2Metric::from_str(&test_cgroup_files).unwrap();
        assert_eq!(result.name, "");
        assert_eq!(result.time_used_tot, 0 as u64);
        assert_eq!(result.time_used_user_mode, 0 as u64);
        assert_eq!(result.time_used_system_mode, 0 as u64);
        assert_eq!(result.anon_used_mem, 0 as u64);
        assert_eq!(result.file_mem, 0 as u64);
        assert_eq!(result.kernel_mem, 0 as u64);
        assert_eq!(result.pagetables_mem, 0 as u64);
    }
}
