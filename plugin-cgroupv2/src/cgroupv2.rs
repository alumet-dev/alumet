use alumet::{
    metrics::{MetricCreationError, TypedMetricId},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};
use anyhow::{Context, Result};
use std::str::FromStr;

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

#[derive(Debug, PartialEq, Clone)]
pub struct CgroupMeasurements {
    /// Name of a Kubernetes pod.
    pub pod_name: String,
    /// Unique identification of a Kubernetes pod.
    pub pod_uid: String,
    /// Resources isolation of a Kubernetes pod.
    pub namespace: String,
    /// Kubernetes pod node.
    pub node: String,
    /// Total CPU usage time by the cgroup.
    pub cpu_time_total: u64,
    /// CPU in user mode usage time by the cgroup.
    pub cpu_time_user_mode: u64,
    /// CPU in system mode usage time by the cgroup.
    pub cpu_time_system_mode: u64,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: u64,
    // Files memory, corresponding to open files and descriptors.
    pub memory_file: u64,
    // Memory reserved for kernel operations.
    pub memory_kernel: u64,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: u64,
}

impl FromStr for CgroupMeasurements {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut cgroup_struc_to_ret = CgroupMeasurements {
            pod_name: "".to_owned(),
            pod_uid: "".to_owned(),
            namespace: "".to_owned(),
            node: "".to_owned(),
            cpu_time_total: 0,
            cpu_time_user_mode: 0,
            cpu_time_system_mode: 0,
            memory_anonymous: 0,
            memory_file: 0,
            memory_kernel: 0,
            memory_pagetables: 0,
        };

        for line in s.lines() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() >= 2 {
                let value = parts[1]
                    .parse::<u64>()
                    .with_context(|| format!("Parsing of value : {}", parts[1]))?;
                match parts[0] {
                    "usage_usec" => cgroup_struc_to_ret.cpu_time_total = value,
                    "user_usec" => cgroup_struc_to_ret.cpu_time_user_mode = value,
                    "system_usec" => cgroup_struc_to_ret.cpu_time_system_mode = value,
                    "anon" => cgroup_struc_to_ret.memory_anonymous = value,
                    "file" => cgroup_struc_to_ret.memory_file = value,
                    "kernel_stack" => cgroup_struc_to_ret.memory_kernel = value,
                    "pagetables" => cgroup_struc_to_ret.memory_pagetables = value,
                    _ => continue,
                }
            }
        }
        Ok(cgroup_struc_to_ret)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Metrics {
    /// Total CPU usage time by the cgroup.
    pub cpu_time_total: TypedMetricId<u64>,
    /// CPU in user mode usage time by the cgroup.
    pub cpu_time_user_mode: TypedMetricId<u64>,
    /// CPU in system mode usage time by the cgroup.
    pub cpu_time_system_mode: TypedMetricId<u64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: TypedMetricId<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub memory_file: TypedMetricId<u64>,
    /// Memory reserved for kernel operations.
    pub memory_kernel: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: TypedMetricId<u64>,
    /// Total memory used by cgroup.
    pub memory_total: TypedMetricId<u64>,
}

impl Metrics {
    /// Provides a information base to create metric before sending CPU and memory data,
    /// with `name`, `unit` and `description` parameters.
    ///
    /// # Arguments
    ///
    /// * `alumet` - A AlumetPluginStart structure passed to plugins for the start-up phase.
    ///
    /// # Error
    ///
    ///  Return `MetricCreationError` when an error occur during creation a new metric.
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let usec = PrefixedUnit::micro(Unit::Second);

        Ok(Self {
            // CPU cgroup data
            cpu_time_total: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_total",
                usec.clone(),
                "Total CPU usage time by the cgroup",
            )?,
            cpu_time_user_mode: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_user",
                usec.clone(),
                "CPU in user mode usage time by the cgroup",
            )?,
            cpu_time_system_mode: alumet.create_metric::<u64>(
                "cgroup_cpu_usage_system",
                usec.clone(),
                "CPU in system mode usage time by the cgroup",
            )?,

            // Memory cgroup data
            memory_anonymous: alumet.create_metric::<u64>(
                "cgroup_memory_anonymous",
                Unit::Byte.clone(),
                "Anonymous used memory, corresponding to running process and various allocated memory",
            )?,
            memory_file: alumet.create_metric::<u64>(
                "cgroup_memory_file",
                Unit::Byte.clone(),
                "Files memory, corresponding to open files and descriptors",
            )?,
            memory_kernel: alumet.create_metric::<u64>(
                "cgroup_memory_kernel_stack",
                Unit::Byte.clone(),
                "Memory reserved for kernel operations",
            )?,
            memory_pagetables: alumet.create_metric::<u64>(
                "cgroup_memory_pagetables",
                Unit::Byte.clone(),
                "Memory used to manage correspondence between virtual and physical addresses",
            )?,
            memory_total: alumet.create_metric::<u64>(
                "cgroup_memory_total",
                Unit::Byte.clone(),
                "Total memory used by cgroup",
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test `from_str` function with in extracted result,
    // a negative value to test representation
    #[test]
    fn test_signed_values() {
        let str_cpu = "
            usage_usec -10000
            user_usec -20000
            system_usec -30000";

        let str_memory = "
            anon -10000
            file -20000
            kernel_stack -30000
            pagetables -40000
            percpu 890784
            sock 16384
            shmem 2453504
            file_mapped -50000
            ....";

        CgroupMeasurements::from_str(str_cpu).expect_err("ERROR : Signed value");
        CgroupMeasurements::from_str(str_memory).expect_err("ERROR : Signed value");
    }

    // Test `from_str` function with in extracted result,
    // a float or decimal value
    #[test]
    fn test_double_values() {
        let str_cpu = "
            usage_usec 10000.05
            user_usec 20000.25
            system_usec 30000.33";

        let str_memory = "
            anon 10000.05
            file 20000.25
            kernel_stack 30000.33
            pagetables 124325768932.56";

        CgroupMeasurements::from_str(str_cpu).expect_err("ERROR : Decimal value");
        CgroupMeasurements::from_str(str_memory).expect_err("ERROR : Decimal value");
    }

    // Test `from_str` function with in extracted result,
    // a null, empty or incompatible string
    #[test]
    fn test_invalid_values() {
        let str_cpu = "
            usage_usec !#⚠
            user_usec
            system_usec -123abc";

        let str_memory = "
            anon !#⚠
            file
            pagetables -123abc
            ...";

        CgroupMeasurements::from_str(str_cpu).expect_err("ERROR : Incompatible value");
        CgroupMeasurements::from_str(str_memory).expect_err("ERROR : Incompatible value");
    }

    // Test `from_str` function with in extracted result,
    // an empty string
    #[test]
    fn test_empty_values() {
        let str: &str = "";
        let result = CgroupMeasurements::from_str(str).unwrap();
        // Memory file str
        assert_eq!(result.memory_anonymous, 0);
        assert_eq!(result.memory_file, 0);
        assert_eq!(result.memory_kernel, 0);
        assert_eq!(result.memory_pagetables, 0);
        // CPU file str
        assert_eq!(result.cpu_time_total, 0);
        assert_eq!(result.cpu_time_user_mode, 0);
        assert_eq!(result.cpu_time_system_mode, 0);
    }

    // Test for calculating `mem_total` with structure parameters
    #[test]
    fn test_calc_mem() {
        let result = CgroupMeasurements {
            pod_name: "".to_owned(),
            pod_uid: "test_pod_uid".to_owned(),
            namespace: "test_pod_namespace".to_owned(),
            node: "test_pod_node".to_owned(),
            cpu_time_total: 64,
            cpu_time_user_mode: 16,
            cpu_time_system_mode: 32,
            memory_anonymous: 1024,
            memory_file: 256,
            memory_kernel: 4096,
            memory_pagetables: 512,
        };

        let expected = CgroupMeasurements {
            pod_name: "".to_owned(),
            pod_uid: "test_pod_uid".to_owned(),
            namespace: "test_pod_namespace".to_owned(),
            node: "test_pod_node".to_owned(),
            cpu_time_total: 64,
            cpu_time_user_mode: 16,
            cpu_time_system_mode: 32,
            memory_anonymous: 1024,
            memory_file: 256,
            memory_kernel: 4096,
            memory_pagetables: 512,
        };

        assert_eq!(result, expected);

        let mem_total = result.memory_anonymous + result.memory_file + result.memory_kernel + result.memory_pagetables;
        assert_eq!(mem_total, 5888);
    }
}
