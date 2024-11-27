//! # cgroupv2 file for k8s and oar3 module
//!
//! This module provides functionality for formatting metrics.
use alumet::{
    metrics::{MetricCreationError, TypedMetricId},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};
use anyhow::{Context, Result};
use std::str::FromStr;

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

#[derive(Debug, PartialEq, Clone)]
pub struct CgroupV2Metric {
    /// Name of a Kubernetes pod.
    pub name: String,
    /// Unique identification of a Kubernetes pod.
    pub uid: String,
    /// Resources isolation of a Kubernetes pod.
    pub namespace: String,
    /// Kubernetes pod node.
    pub node: String,
    /// Total CPU usage time by the cgroup.
    pub time_used_tot: u64,
    /// CPU in user mode usage time by the cgroup.
    pub time_used_user_mode: u64,
    /// CPU in system mode usage time by the cgroup.
    pub time_used_system_mode: u64,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub anon_used_mem: u64,
    // Files memory, corresponding to open files and descriptors.
    pub file_mem: u64,
    // Memory reserved for kernel operations.
    pub kernel_mem: u64,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub pagetables_mem: u64,
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
        };

        for line in s.lines() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() >= 2 {
                let value = parts[1]
                    .parse::<u64>()
                    .context(format!("ERROR : Parsing of value : {}", parts[1]))?;
                {
                    match parts[0] {
                        "usage_usec" => cgroup_struc_to_ret.time_used_tot = value,
                        "user_usec" => cgroup_struc_to_ret.time_used_user_mode = value,
                        "system_usec" => cgroup_struc_to_ret.time_used_system_mode = value,
                        "anon" => cgroup_struc_to_ret.anon_used_mem = value,
                        "file" => cgroup_struc_to_ret.file_mem = value,
                        "kernel_stack" => cgroup_struc_to_ret.kernel_mem = value,
                        "pagetables" => cgroup_struc_to_ret.pagetables_mem = value,
                        _ => continue,
                    }
                }
            }
        }
        Ok(cgroup_struc_to_ret)
    }
}

#[derive(Clone)]
pub struct Metrics {
    /// Total CPU usage time by the cgroup.
    pub time_used_tot: TypedMetricId<u64>,
    /// CPU in user mode usage time by the cgroup.
    pub time_used_user_mode: TypedMetricId<u64>,
    /// CPU in system mode usage time by the cgroup.
    pub time_used_system_mode: TypedMetricId<u64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub anon_used_mem: TypedMetricId<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub file_mem: TypedMetricId<u64>,
    /// Memory reserved for kernel operations.
    pub kernel_mem: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub pagetables_mem: TypedMetricId<u64>,
    /// Total memory used by cgroup.
    pub total_mem: TypedMetricId<u64>,
}

#[cfg(not(tarpaulin_include))]
impl Metrics {
    /// Provides a information base to create metric before sending CPU and memory data,
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

// ------------------ //
// --- UNIT TESTS --- //
// ------------------ //
#[cfg(test)]
mod tests {
    use super::*;

    // Test `from_str` function with in extracted result,
    // a null value
    #[test]
    fn test_zero_values() {
        let str: &str = "
            usage_usec 0
            user_usec 0
            system_usec 0
            nr_periods 0
            nr_throttled 0
            throttled_usec 0";

        let result: CgroupV2Metric = CgroupV2Metric::from_str(str).unwrap();
        assert_eq!(result.time_used_tot, 0);
        assert_eq!(result.time_used_user_mode, 0);
        assert_eq!(result.time_used_system_mode, 0);
    }

    // Test `from_str` function with in extracted result,
    // a big value to test overflow
    #[test]
    fn test_large_values() {
        let str: &str = "
            anon 18446744073709551615
            file 18446744073709551615
            kernel_stack 18446744073709551615
            pagetables 18446744073709551615
            percpu 890784
            sock 16384
            shmem 2453504
            file_mapped 72806400
            ....";

        let result: CgroupV2Metric = CgroupV2Metric::from_str(str).unwrap();
        assert_eq!(result.anon_used_mem, 18446744073709551615);
        assert_eq!(result.file_mem, 18446744073709551615);
        assert_eq!(result.kernel_mem, 18446744073709551615);
        assert_eq!(result.pagetables_mem, 18446744073709551615);
    }

    // Test `from_str` function with in extracted result,
    // a negative value to test representation
    #[test]
    fn test_signed_values() {
        let str: &str = "
            usage_usec 10000
            user_usec -20000
            system_usec -30000";

        CgroupV2Metric::from_str(str).expect_err("ERROR : Signed value");
    }

    // Test `from_str` function with in extracted result,
    // a float or decimal value
    #[test]
    fn test_double_values() {
        let str: &str = "
            anon 10000.05
            file 20000.25
            kernel_stack 30000.33
            pagetables 124325768932.56";

        CgroupV2Metric::from_str(str).expect_err("ERROR : Decimal value");
    }

    // Test `from_str` function with in extracted result,
    // a null, empty or incompatible string
    #[test]
    fn test_invalid_values() {
        let str: &str = "
            anon !#⚠
            file
            pagetables -123abc
            ...";

        CgroupV2Metric::from_str(str).expect_err("ERROR : Incompatible value");
    }

    // Test `from_str` function with in extracted result,
    // an empty string
    #[test]
    fn test_empty_values() {
        let str: &str = "";
        let result: CgroupV2Metric = CgroupV2Metric::from_str(str).unwrap();
        // Memory file str
        assert_eq!(result.anon_used_mem, 0);
        assert_eq!(result.file_mem, 0);
        assert_eq!(result.kernel_mem, 0);
        assert_eq!(result.pagetables_mem, 0);
        // CPU file str
        assert_eq!(result.time_used_tot, 0);
        assert_eq!(result.time_used_user_mode, 0);
        assert_eq!(result.time_used_system_mode, 0);
    }

    // Test `from_str` function with in extracted result,
    // a null string
    #[test]
    fn test_null_values() {
        let null_str: Option<&str> = None;

        let result: Result<CgroupV2Metric, _> = match null_str {
            Some(s) => CgroupV2Metric::from_str(s),
            None => Ok(CgroupV2Metric {
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
            }),
        };

        let expected: CgroupV2Metric = CgroupV2Metric {
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
        };

        assert_eq!(result.unwrap(), expected);
    }

    // Test for calculating `mem_total` with structure parameters
    #[test]
    fn test_calc_mem() {
        let result: CgroupV2Metric = CgroupV2Metric {
            name: "".to_owned(),
            uid: "test_pod_uid".to_owned(),
            namespace: "test_pod_namespace".to_owned(),
            node: "test_pod_node".to_owned(),
            time_used_tot: 0,
            time_used_user_mode: 0,
            time_used_system_mode: 0,
            anon_used_mem: 1024,
            file_mem: 256,
            kernel_mem: 4096,
            pagetables_mem: 512,
        };
        assert_eq!(result.name, "");
        assert_eq!(result.uid, "test_pod_uid");
        assert_eq!(result.namespace, "test_pod_namespace");
        assert_eq!(result.node, "test_pod_node");

        let mem_total: u64 = result.anon_used_mem + result.file_mem + result.kernel_mem + result.pagetables_mem;
        assert_eq!(mem_total, 5888);
    }
}
