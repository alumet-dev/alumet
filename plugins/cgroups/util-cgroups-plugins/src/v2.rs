use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, MeasurementType, Timestamp},
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};
use util_cgroups::{
    Cgroup,
    measure::v2::{V2Collector, cpu::CpuStatCollectorSettings, memory::MemoryStatCollectorSettings},
};

use super::{
    delta::CpuDeltaCounters, metrics::AugmentedMetric, metrics::AugmentedMetrics, self_stop::analyze_io_result,
};

pub struct CgroupV2Probe {
    consumer: ResourceConsumer,
    delta_counters: CpuDeltaCounters,
    metrics: AugmentedMetrics,
    collector: V2Collector,
    io_buf: Vec<u8>,
    last_timestamp: Option<Timestamp>,
    n_cores: usize,
}

impl CgroupV2Probe {
    pub fn new(cgroup: Cgroup<'_>, metrics: AugmentedMetrics) -> anyhow::Result<Self> {
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

        // To get the number of logical core, one could think about calling num_cpus::get().
        // However, this is affected by the constraints set on the Alumet process (sched affinity, cgroups cpuset), which is not what we want.
        let n_cores = crate::cpus::online_cpus()?.len();

        Ok(Self {
            consumer,
            delta_counters: Default::default(),
            metrics,
            collector,
            io_buf,
            last_timestamp: None,
            n_cores,
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
        // To compute the cpu utilization as a percentage, we need to :
        // - compute the difference of the current counter value with the previous one (the value is in microseconds)
        // - divide it by the elapsed time (in microseconds)
        let last_timestamp = self.last_timestamp;
        self.last_timestamp = Some(t);
        let poll_interval_micros = match last_timestamp {
            Some(last) => Some(t.duration_since(last)?.as_micros()),
            None => None,
        };
        let data = analyze_io_result(self.collector.measure(&mut self.io_buf))?;

        // The data that we collect here is cumulative across all the CPU cores.
        // => We compute a percentage for the whole machine.
        let resource = Resource::LocalMachine;

        // CPU statistics
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
                if let Some(poll_interval) = poll_interval_micros {
                    measurements.push(
                        self.new_point(
                            &self.metrics.cpu_percent,
                            t,
                            &resource,
                            (value as f64 / poll_interval as f64 / self.n_cores as f64) * 100.0,
                        )
                        .with_attr("kind", "total"),
                    );
                }
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
                if let Some(poll_interval) = poll_interval_micros {
                    measurements.push(
                        self.new_point(
                            &self.metrics.cpu_percent,
                            t,
                            &resource,
                            (value as f64 / poll_interval as f64 / self.n_cores as f64) * 100.0,
                        )
                        .with_attr("kind", "system"),
                    );
                }
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
                if let Some(poll_interval) = poll_interval_micros {
                    measurements.push(
                        self.new_point(
                            &self.metrics.cpu_percent,
                            t,
                            &resource,
                            (value as f64 / poll_interval as f64 / self.n_cores as f64) * 100.0,
                        )
                        .with_attr("kind", "user"),
                    );
                }
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

    fn reset(&mut self) -> anyhow::Result<()> {
        self.delta_counters.reset();
        self.last_timestamp = None;
        Ok(())
    }
}
