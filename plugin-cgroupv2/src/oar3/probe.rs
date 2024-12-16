use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::{CounterDiff, CounterDiffUpdate},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use super::utils::{gather_value, CgroupV2MetricFile};
use crate::cgroupv2::{CgroupV2Metric, Metrics};

pub struct CgroupV2prob {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
    pub anon_used_mem: TypedMetricId<u64>,
    pub file_mem: TypedMetricId<u64>,
    pub kernel_mem: TypedMetricId<u64>,
    pub pagetables_mem: TypedMetricId<u64>,
    pub total_mem: TypedMetricId<u64>,
}

#[cfg(not(tarpaulin_include))]
impl CgroupV2prob {
    pub fn new(
        metric: Metrics,
        metric_file: CgroupV2MetricFile,
        counter_tot: CounterDiff,
        counter_sys: CounterDiff,
        counter_usr: CounterDiff,
    ) -> anyhow::Result<CgroupV2prob> {
        Ok(CgroupV2prob {
            cgroup_v2_metric_file: metric_file,
            time_tot: counter_tot,
            time_usr: counter_usr,
            time_sys: counter_sys,
            time_used_tot: metric.time_used_tot,
            time_used_system_mode: metric.time_used_system_mode,
            time_used_user_mode: metric.time_used_user_mode,
            anon_used_mem: metric.anon_used_mem,
            file_mem: metric.file_mem,
            kernel_mem: metric.kernel_mem,
            pagetables_mem: metric.pagetables_mem,
            total_mem: metric.total_mem,
        })
    }

    /// Create a measurement point with given value,
    /// the `LocalMachine` resource and some attributes related to the pod.
    fn create_measurement_point(
        &self,
        timestamp: Timestamp,
        metric_id: TypedMetricId<u64>,
        resource_consumer: ResourceConsumer,
        value_measured: u64,
        metrics_param: &CgroupV2Metric,
    ) -> MeasurementPoint {
        MeasurementPoint::new(
            timestamp,
            metric_id,
            Resource::LocalMachine,
            resource_consumer,
            value_measured,
        )
        .with_attr("name", AttributeValue::String(metrics_param.uid.clone()))
    }
}

#[cfg(not(tarpaulin_include))]
impl alumet::pipeline::Source for CgroupV2prob {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut buffer = String::new();
        let metrics: CgroupV2Metric = gather_value(&mut self.cgroup_v2_metric_file, &mut buffer)?;

        let diff_tot = match self.time_tot.update(metrics.time_used_tot) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) | CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_usr = match self.time_usr.update(metrics.time_used_user_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_sys = match self.time_sys.update(metrics.time_used_system_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };

        let consumer_cpu = ResourceConsumer::ControlGroup {
            path: (self.cgroup_v2_metric_file.path.to_string_lossy().to_string().into()),
        };

        let consumer_memory = ResourceConsumer::ControlGroup {
            path: (self.cgroup_v2_metric_file.path.to_string_lossy().to_string().into()),
        };

        // Push cpu total usage measure for user and system
        if let Some(value_tot) = diff_tot {
            let p_tot =
                self.create_measurement_point(timestamp, self.time_used_tot, consumer_cpu.clone(), value_tot, &metrics);
            measurements.push(p_tot);
        }

        // Push cpu usage measure for user
        if let Some(value_usr) = diff_usr {
            let p_usr = self.create_measurement_point(
                timestamp,
                self.time_used_user_mode,
                consumer_cpu.clone(),
                value_usr,
                &metrics,
            );
            measurements.push(p_usr);
        }

        // Push cpu usage measure for system
        if let Some(value_sys) = diff_sys {
            let p_sys = self.create_measurement_point(
                timestamp,
                self.time_used_system_mode,
                consumer_cpu.clone(),
                value_sys,
                &metrics,
            );
            measurements.push(p_sys);
        }

        // Push anonymous used memory measure corresponding to running process and various allocated memory
        let mem_anon_value = metrics.anon_used_mem;
        let m_anon = self.create_measurement_point(
            timestamp,
            self.anon_used_mem,
            consumer_memory.clone(),
            mem_anon_value / 1000,
            &metrics,
        );
        measurements.push(m_anon);

        // Push files memory measure, corresponding to open files and descriptors
        let mem_file_value = metrics.file_mem;
        let m_file = self.create_measurement_point(
            timestamp,
            self.file_mem,
            consumer_memory.clone(),
            mem_file_value / 1000,
            &metrics,
        );
        measurements.push(m_file);

        // Push kernel memory measure
        let mem_kernel_value = metrics.kernel_mem;
        let m_ker = self.create_measurement_point(
            timestamp,
            self.kernel_mem,
            consumer_memory.clone(),
            mem_kernel_value / 1000,
            &metrics,
        );
        measurements.push(m_ker);

        // Push pagetables memory measure
        let mem_pagetables_value = metrics.pagetables_mem;
        let m_pgt = self.create_measurement_point(
            timestamp,
            self.pagetables_mem,
            consumer_memory.clone(),
            mem_pagetables_value / 1000,
            &metrics,
        );
        measurements.push(m_pgt);

        // Push total memory used by cgroup measure
        let mem_total_value = (mem_anon_value + mem_file_value + mem_kernel_value + mem_pagetables_value) / 1000;
        let m_tot = self.create_measurement_point(
            timestamp,
            self.total_mem,
            consumer_memory.clone(),
            mem_total_value,
            &metrics,
        );
        measurements.push(m_tot);

        Ok(())
    }
}
