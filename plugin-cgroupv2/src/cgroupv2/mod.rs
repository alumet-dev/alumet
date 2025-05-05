use alumet::{
    measurement::{AttributeValue, MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
    pipeline::elements::source::error::PollError,
    plugin::AlumetPluginStart,
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Result;
use cpu_stat::CpuStatAlumetProbe;
use memory_current::MemoryCurrentAlumetProbe;
use memory_stat::MemoryStatAlumetProbe;
use std::path::PathBuf;

mod cpu_stat;
mod memory_current;
mod memory_stat;

#[cfg(test)]
pub mod tests_mock;

/// Cgroupv2Probe is a high level component that manage the collection of one cgroupv2 group measurements and adapt it to Alumet interfaces, by gathering controller files collections.
pub struct Cgroupv2Probe {
    pub cpu_stat: Option<CpuStatAlumetProbe>,
    pub memory_stat: Option<MemoryStatAlumetProbe>,
    pub memory_current: Option<MemoryCurrentAlumetProbe>,
    // could be extended to manage other cgroupv2 controller files.
}

/// Metrics structure gather informations about the Alumet metrics shared by all cgroupv2 related submodules (k8s, oar3, etc.).
/// In particular it contains metric name, metric unit, metric descriptions in order to harmonize how submodules share metrics using cgroupv2.
#[derive(Clone, Eq, PartialEq)]
pub struct Metrics {
    /// Total CPU usage time by the cgroup since last measurement
    cpu_time_delta: TypedMetricId<u64>,
    /// Memory currently used by the cgroup.
    memory_usage: TypedMetricId<u64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    memory_anonymous: TypedMetricId<u64>,
    /// Files memory, corresponding to open files and descriptors.
    memory_file: TypedMetricId<u64>,
    /// Memory reserved for kernel operations.
    memory_kernel: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    memory_pagetables: TypedMetricId<u64>,
}

impl Cgroupv2Probe {
    /// new_from_cgroup_dir factory creates a new Cgroupv2Probe in an easy way by passing the path to the cgroup base directory.
    /// note that it will provide a Cgroupv2Probe that enable the collections of all controller files and all file measurements.
    /// if you want to have a better control on the configuration of the Cgroupv2Probe, in particular if you want to enable/disable the collection of one controller file or specific file measurements, you should use the `new` factory function.
    /// note that you can extend metrics attributes using add_additional_attrs method.
    pub fn new_from_cgroup_dir(cgroup_dir: PathBuf, metrics: Metrics) -> Result<Self, anyhow::Error> {
        let cpu_stat_probe = CpuStatAlumetProbe::new(
            cgroup_dir
                .clone()
                .join("cpu.stat")
                .into_os_string()
                .into_string()
                .unwrap(),
            true,
            true,
            true,
            Some(metrics.cpu_time_delta),
            Some(metrics.cpu_time_delta),
            Some(metrics.cpu_time_delta),
            Some(vec![("kind".to_string(), AttributeValue::Str("total"))]),
            Some(vec![("kind".to_string(), AttributeValue::Str("user"))]),
            Some(vec![("kind".to_string(), AttributeValue::Str("system"))]),
        )?;

        let memory_stat_probe = MemoryStatAlumetProbe::new(
            cgroup_dir
                .clone()
                .join("memory.stat")
                .into_os_string()
                .into_string()
                .unwrap(),
            true,
            true,
            true,
            true,
            Some(metrics.memory_anonymous),
            Some(metrics.memory_file),
            Some(metrics.memory_kernel),
            Some(metrics.memory_pagetables),
            None,
            None,
            None,
            None,
        )?;

        let memory_current_probe = MemoryCurrentAlumetProbe::new(
            cgroup_dir
                .clone()
                .join("memory.current")
                .into_os_string()
                .into_string()
                .unwrap(),
            metrics.memory_usage,
            Some(vec![("kind".to_string(), AttributeValue::Str("resident"))]),
        )?;
        Ok(Self::new(
            Some(cpu_stat_probe),
            Some(memory_stat_probe),
            Some(memory_current_probe),
        )?)
    }

    /// new factory creates a new Cgroupv2Probe.
    /// it takes controller files components as parameter that are optional meaning that a None one will disable the collection of this controller file.
    /// note that you can extend metrics attributes using add_additional_attrs method.
    pub fn new(
        cpu_stat: Option<CpuStatAlumetProbe>,
        memory_stat: Option<MemoryStatAlumetProbe>,
        memory_current: Option<MemoryCurrentAlumetProbe>,
    ) -> Result<Self, anyhow::Error> {
        Ok(Self {
            cpu_stat,
            memory_stat,
            memory_current,
        })
    }

    pub fn add_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        if let Some(cpu_stat) = &mut self.cpu_stat {
            cpu_stat.add_additional_attrs(attributes.clone());
        }
        if let Some(memory_stat) = &mut self.memory_stat {
            memory_stat.add_additional_attrs(attributes.clone());
        }
        if let Some(memory_current) = &mut self.memory_current {
            memory_current.add_additional_attrs(attributes.clone());
        }
    }

    pub fn collect_measurements(&mut self, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
        let mut measurement_points = Vec::<MeasurementPoint>::new();

        // note/todo: collections could be parallelized
        if let Some(ref mut cpu_stat) = self.cpu_stat {
            measurement_points.extend(cpu_stat.collect_measurements(timestamp)?);
        }

        if let Some(ref mut memory_stat) = self.memory_stat {
            measurement_points.extend(memory_stat.collect_measurements(timestamp)?);
        }

        if let Some(ref mut memory_current) = self.memory_current {
            measurement_points.extend(memory_current.collect_measurements(timestamp)?);
        }

        Ok(measurement_points)
    }
}

impl Metrics {
    /// new factory creates the Metrics structure by instantiating metrics that will be managed by Alumet
    /// It provides common `name`, `unit` and `description` across all submodules using the cgroupv2 crate.
    ///
    /// # Arguments
    ///
    /// * `alumet` - A AlumetPluginStart structure passed to plugins for the start-up phase.
    ///
    /// # Error
    ///
    ///  Return `MetricCreationError` when an error occur during creation a new metric.
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            cpu_time_delta: alumet.create_metric::<u64>(
                "cpu_time_delta",
                PrefixedUnit::nano(Unit::Second),
                "Total CPU usage time by the cgroup since last measurement",
            )?,

            // Memory cgroup data
            memory_usage: alumet.create_metric::<u64>(
                "memory_usage",
                Unit::Byte.clone(),
                "Memory currently used by the cgroup",
            )?,
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
        })
    }
}

fn measurement_to_point(
    timestamp: Timestamp,
    metric: TypedMetricId<u64>,
    consumer: ResourceConsumer,
    value: u64,
    additional_attrs: Option<Vec<(String, AttributeValue)>>,
) -> MeasurementPoint {
    MeasurementPoint::new(timestamp, metric, Resource::LocalMachine, consumer, value)
        .with_attr_vec(additional_attrs.unwrap_or(Vec::new()))
}

fn add_additional_attrs(target: &mut Option<Vec<(String, AttributeValue)>>, attributes: Vec<(String, AttributeValue)>) {
    target.get_or_insert(Vec::new()).extend(attributes);
}
