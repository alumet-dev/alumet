use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    pipeline::{elements::error::PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use util_cgroups::{measure::v1::V1Collector, Cgroup};

use super::{
    delta::CpuDeltaCounters, metrics::AugmentedMetric, metrics::AugmentedMetrics, self_stop::analyze_io_result,
};

pub struct CgroupV1Probe {
    consumer: ResourceConsumer,
    delta_counters: CpuDeltaCounters,
    metrics: AugmentedMetrics,
    collector: V1Collector,
    io_buf: Vec<u8>,
}

impl CgroupV1Probe {
    pub fn new<'h>(cgroup: Cgroup<'h>, metrics: AugmentedMetrics) -> anyhow::Result<Self> {
        let cgroup_canon_path = cgroup.canonical_path().to_owned();
        let consumer = ResourceConsumer::ControlGroup {
            path: cgroup_canon_path.clone().into(),
        };
        let io_buf = Vec::new();
        let collector = V1Collector::in_single_hierarchy(cgroup)?;
        Ok(Self {
            consumer,
            delta_counters: Default::default(),
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

impl Source for CgroupV1Probe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        let data = analyze_io_result(self.collector.measure(&mut self.io_buf))?;
        let resource = Resource::LocalMachine; // TODO more precise, but we don't know the pkg id

        // Cpu statistics
        if let Some(value) = data
            .cpuacct_usage
            .map(|v| self.delta_counters.usage.update(v).difference())
            .flatten()
        {
            measurements.push(
                self.new_point(&self.metrics.cpu_time_delta, t, &resource, value)
                    .with_attr("kind", "total"),
            );
        }

        // Memory statistics
        if let Some(mem) = data.memory_stat {
            measurements.push(self.new_point(&self.metrics.memory_usage, t, &resource, mem));
        }
        Ok(())
    }
}
