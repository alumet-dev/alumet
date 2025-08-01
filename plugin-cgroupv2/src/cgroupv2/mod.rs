use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
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

/// Metrics structure contains common metrics shared by all cgroupv2 related submodules (k8s, oar3, etc.).
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
    memory_kernel_stack: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    memory_pagetables: TypedMetricId<u64>,
}

/// MeasurementAlumetMapping is used by cgroupv2 sub-probes to configure how measurement will be mapped to an Alumet metric
pub struct MeasurementAlumetMapping {
    pub metric: TypedMetricId<u64>,
    pub additional_attrs: Option<Vec<(String, AttributeValue)>>,
}

impl MeasurementAlumetMapping {
    fn new(metric: TypedMetricId<u64>) -> Self {
        Self {
            metric,
            additional_attrs: None,
        }
    }
    fn add_additional_attrs(mut self, additional_attrs: Vec<(String, AttributeValue)>) -> Self {
        self.additional_attrs = Some(additional_attrs);
        self
    }
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
            Some(
                MeasurementAlumetMapping::new(metrics.cpu_time_delta)
                    .add_additional_attrs(vec![("kind".to_string(), AttributeValue::Str("total"))]),
            ),
            Some(
                MeasurementAlumetMapping::new(metrics.cpu_time_delta)
                    .add_additional_attrs(vec![("kind".to_string(), AttributeValue::Str("user"))]),
            ),
            Some(
                MeasurementAlumetMapping::new(metrics.cpu_time_delta)
                    .add_additional_attrs(vec![("kind".to_string(), AttributeValue::Str("system"))]),
            ),
        )?;

        let memory_stat_probe = MemoryStatAlumetProbe::new(
            cgroup_dir
                .clone()
                .join("memory.stat")
                .into_os_string()
                .into_string()
                .unwrap(),
            Some(MeasurementAlumetMapping::new(metrics.memory_anonymous)),
            Some(MeasurementAlumetMapping::new(metrics.memory_file)),
            Some(MeasurementAlumetMapping::new(metrics.memory_kernel_stack)),
            Some(MeasurementAlumetMapping::new(metrics.memory_pagetables)),
        )?;

        let memory_current_probe = MemoryCurrentAlumetProbe::new(
            cgroup_dir
                .clone()
                .join("memory.current")
                .into_os_string()
                .into_string()
                .unwrap(),
            MeasurementAlumetMapping::new(metrics.memory_usage)
                .add_additional_attrs(vec![("kind".to_string(), AttributeValue::Str("resident"))]),
        )?;
        Ok(Self {
            cpu_stat: Some(cpu_stat_probe),
            memory_stat: Some(memory_stat_probe),
            memory_current: Some(memory_current_probe),
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

    pub fn collect_measurements(
        &mut self,
        timestamp: Timestamp,
        measurements: &mut MeasurementAccumulator,
    ) -> Result<(), anyhow::Error> {
        if let Some(ref mut cpu_stat) = self.cpu_stat {
            cpu_stat.collect_measurements(timestamp, measurements)?;
        }

        if let Some(ref mut memory_stat) = self.memory_stat {
            memory_stat.collect_measurements(timestamp, measurements)?;
        }

        if let Some(ref mut memory_current) = self.memory_current {
            memory_current.collect_measurements(timestamp, measurements)?;
        }

        Ok(())
    }
}

impl Metrics {
    /// Registers common metrics related to cgroupv2 in Alumet.
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
            memory_kernel_stack: alumet.create_metric::<u64>(
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
        .with_attr_vec(additional_attrs.unwrap_or_default())
}

fn add_additional_attrs(target: &mut Option<Vec<(String, AttributeValue)>>, attributes: Vec<(String, AttributeValue)>) {
    target.get_or_insert(Vec::new()).extend(attributes);
}
