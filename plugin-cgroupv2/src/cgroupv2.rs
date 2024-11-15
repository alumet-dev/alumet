//! # cgroupv2 file for k8s and oar3 module
//!
//! This module provides functionality to formating metrics.
//!

use std::str::FromStr;

use alumet::{
    metrics::{MetricCreationError, TypedMetricId},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

/// # Structure
/// 
/// * `CgroupV2Metric` - Public structure storing identifier of each CPU and memory data
///
/// # Parameters
///
/// * `name` - Name of a Kubernetes pod
/// * `uid` - Identification of a Kubernetes pod
/// * `namespace` - Path name of a Kubernetes pod
/// * `node` - Kubernetes pod node
/// * `time_used_tot` - Total CPU usage time by the cgroup
/// * `time_used_user_mode` - CPU in user mode usage time by the cgroup
/// * `time_used_system_mode` - CPU in system mode usage time by the cgroup
/// * `anon_used_mem` - Anonyme used memory, corresponding to running process and various allocated memory
/// * `file_mem` - Files memory, corresponding to open files and descriptors
/// * `shared_mem` - Interprocess communication shared memory
/// * `file_mapped_mem` - Mapped files in memory
/// * `total_mem` - Total memory used by cgroup
/// 
#[derive(Debug, PartialEq, Clone)]
pub struct CgroupV2Metric {
    pub name: String,
    pub uid: String,
    pub namespace: String,
    pub node: String,
    pub time_used_tot: u64,
    pub time_used_user_mode: u64,
    pub time_used_system_mode: u64,
    pub anon_used_mem: u64,
    pub file_mem: u64,
    pub shared_mem: u64,
    pub file_mapped_mem: u64,
    pub total_mem: u64
}

impl FromStr for CgroupV2Metric {
    type Err = anyhow::Error;
    /// # Function
    /// 
    /// * `from_str` - Function provides functionality to parse a string by whitespaces places.
    ///
    /// # Arguments
    ///
    /// * `s` - String will be parsed.
    ///
    /// # Returns
    ///
    /// * `Result` - Type representing success `Ok` or failure `Err` from parsing.
    ///
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
            shared_mem: 0,
            file_mapped_mem: 0,
            total_mem: 0
        };

        for line in s.lines() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() >= 2 {
                match parts[0] {
                    // Total CPU usage time by the cgroup
                    "usage_usec" => { cgroup_struc_to_ret.time_used_tot = parts[1].parse::<u64>()?; }
                    // CPU in user mode usage time by the cgroup
                    "user_usec" => { cgroup_struc_to_ret.time_used_user_mode = parts[1].parse::<u64>()?; }
                    // CPU in system mode usage time by the cgroup
                    "system_usec" => { cgroup_struc_to_ret.time_used_system_mode = parts[1].parse::<u64>()?; }
                    // Anonyme used memory, corresponding to running process and various allocated memory
                    "anon" => { cgroup_struc_to_ret.anon_used_mem = parts[1].parse::<u64>()?; }
                    // Files memory, corresponding to open files and descriptors
                    "file" => { cgroup_struc_to_ret.file_mem = parts[1].parse::<u64>()?; }
                    // Interprocess communication shared memory
                    "shmem" => { cgroup_struc_to_ret.shared_mem = parts[1].parse::<u64>()?; }
                    // Mapped files in memory measure
                    "file_mapped" => { cgroup_struc_to_ret.file_mapped_mem = parts[1].parse::<u64>()?; }
                    &_ => continue,
                }
            }
        }
        Ok(cgroup_struc_to_ret)
    }
}

/// # Structure
/// 
/// * `Metrics` - Public structure storing identifier of each CPU and memory data
///
/// # Parameters
///
/// * `time_used_tot` - Total CPU usage time by the cgroup
/// * `time_used_user_mode` - CPU in user mode usage time by the cgroup
/// * `time_used_system_mode` - CPU in system mode usage time by the cgroup
/// * `anon_used_mem` - Anonyme used memory, corresponding to running process and various allocated memory
/// * `file_mem` - Files memory, corresponding to open files and descriptors
/// * `shared_mem` - Interprocess communication shared memory
/// * `file_mapped_mem` - Mapped files in memory
/// * `total_mem` - Total memory used by cgroup
///
#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
    pub anon_used_mem: TypedMetricId<u64>,
    pub file_mem: TypedMetricId<u64>,
    pub shared_mem: TypedMetricId<u64>,
    pub file_mapped_mem: TypedMetricId<u64>,
    pub total_mem: TypedMetricId<u64>
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
    /// # Dependencies
    ///
    /// This function relies on the `PrefixedUnit` crate provides base unit and a scale.
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
                "cgroup_memory_anonyme",
                kb.clone(),
                "Anonyme used memory, corresponding to running process and various allocated memory",
            )?,
            file_mem: alumet.create_metric::<u64>(
                "cgroup_memory_file",
                kb.clone(),
                "Files memory, corresponding to open files and descriptors",
            )?,
            shared_mem: alumet.create_metric::<u64>(
                "cgroup_memory_shared",
                kb.clone(),
                "Interprocess communication shared memory",
            )?,
            file_mapped_mem: alumet.create_metric::<u64>(
                "cgroup_memory_file_mapped",
                kb.clone(),
                "Mapped files in memory",
            )?,
            total_mem: alumet.create_metric::<u64>(
                "cgroup_memory_total",
                kb.clone(),
                "Total memory used by cgroup",
            )?
        })
    }
}
