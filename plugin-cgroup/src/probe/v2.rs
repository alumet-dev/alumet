use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    pipeline::{elements::error::PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use util_cgroups::{
    measure::v2::{cpu::CpuStatCollectorSettings, memory::MemoryStatCollectorSettings, V2Collector},
    Cgroup,
};

use crate::probe::{AugmentedMetric, self_stop::analyze_io_result};

use super::{AugmentedMetrics, DeltaCounters};

pub struct CgroupV2Probe {
    consumer: ResourceConsumer,
    delta_counters: DeltaCounters,
    metrics: AugmentedMetrics,
    collector: V2Collector,
    io_buf: Vec<u8>,
}

impl CgroupV2Probe {
    pub fn new<'h>(cgroup: Cgroup<'h>, metrics: AugmentedMetrics) -> anyhow::Result<Self> {
        let consumer = ResourceConsumer::ControlGroup {
            path: cgroup.canonical_path().to_owned().into(),
        };
        let mut io_buf = Vec::new();
        let collector = V2Collector::new(
            cgroup,
            MemoryStatCollectorSettings::default(),
            CpuStatCollectorSettings::default(),
            &mut io_buf,
        )?;
        Ok(Self {
            consumer,
            delta_counters: DeltaCounters::default(),
            metrics,
            collector,
            io_buf,
        })
    }

    fn new_point<T: MeasurementType<T = T>>(
        &self,
        metric: &AugmentedMetric<T>,
        t: Timestamp,
        resource: &Resource,
        value: T,
    ) -> MeasurementPoint {
        MeasurementPoint::new(t, metric.metric, resource.clone(), self.consumer.clone(), value)
            .with_attr_slice(&metric.attributes)
            .with_attr_slice(&self.metrics.common_attrs)
    }
}

impl Source for CgroupV2Probe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        let data = analyze_io_result(self.collector.measure(&mut self.io_buf))?;
        let resource = Resource::LocalMachine; // TODO more precise, but we don't know the pkg id

        // Cpu statistics
        if let Some(cpu_stat) = data.cpu_stat {
            if let Some(value) = cpu_stat
                .usage
                .map(|v| self.delta_counters.usage.update(v).difference())
                .flatten()
            {
                measurements.push(
                    self.new_point(&self.metrics.cpu_time_delta, t, &resource, value)
                        .with_attr("kind", "total"),
                );
            }

            if let Some(value) = cpu_stat
                .system
                .map(|v| self.delta_counters.system.update(v).difference())
                .flatten()
            {
                measurements.push(
                    self.new_point(&self.metrics.cpu_time_delta, t, &resource, value)
                        .with_attr("kind", "system"),
                );
            }

            if let Some(value) = cpu_stat
                .user
                .map(|v| self.delta_counters.user.update(v).difference())
                .flatten()
            {
                measurements.push(
                    self.new_point(&self.metrics.cpu_time_delta, t, &resource, value)
                        .with_attr("kind", "user"),
                );
            }
        }

        // Memory statistics
        if let Some(mem) = data.memory_current {
            measurements.push(self.new_point(&self.metrics.memory_usage, t, &resource, mem));
        }
        if let Some(mem_stat) = data.memory_stat {
            if let Some(value) = mem_stat.anon {
                measurements.push(self.new_point(&self.metrics.memory_anonymous, t, &resource, value));
            }
            if let Some(value) = mem_stat.file {
                measurements.push(self.new_point(&self.metrics.memory_file, t, &resource, value));
            }
            if let Some(value) = mem_stat.kernel_stack {
                measurements.push(self.new_point(&self.metrics.memory_kernel_stack, t, &resource, value));
            }
            if let Some(value) = mem_stat.page_tables {
                measurements.push(self.new_point(&self.metrics.memory_pagetables, t, &resource, value));
            }
        }
        Ok(())
    }
}
