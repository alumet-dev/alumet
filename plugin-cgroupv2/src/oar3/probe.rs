use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::{CounterDiff, CounterDiffUpdate},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use super::utils::{gather_value, CgroupV2MetricFile};
use crate::cgroupv2::{CgroupMeasurements, Metrics};

pub struct CgroupV2prob {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
    pub cpu_time_tot: TypedMetricId<u64>,
    pub cpu_time_user_mode: TypedMetricId<u64>,
    pub cpu_time_system_mode: TypedMetricId<u64>,
    pub memory_anon: TypedMetricId<u64>,
    pub memory_file: TypedMetricId<u64>,
    pub memory_kernel: TypedMetricId<u64>,
    pub memory_pagetables: TypedMetricId<u64>,
    pub memory_total: TypedMetricId<u64>,
}

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
            cpu_time_tot: metric.cpu_time_total,
            cpu_time_system_mode: metric.cpu_time_user_mode,
            cpu_time_user_mode: metric.cpu_time_system_mode,
            memory_anon: metric.memory_anonymous,
            memory_file: metric.memory_file,
            memory_kernel: metric.memory_kernel,
            memory_pagetables: metric.memory_pagetables,
            memory_total: metric.memory_total,
        })
    }
}

impl alumet::pipeline::Source for CgroupV2prob {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        /// Create a measurement point with given value,
        /// the `LocalMachine` resource and some attributes related to the pod.
        fn create_measurement_point(
            timestamp: Timestamp,
            metric_id: TypedMetricId<u64>,
            resource_consumer: ResourceConsumer,
            value_measured: u64,
            metrics_param: &CgroupMeasurements,
        ) -> MeasurementPoint {
            MeasurementPoint::new(
                timestamp,
                metric_id,
                Resource::LocalMachine,
                resource_consumer,
                value_measured,
            )
            .with_attr("name", AttributeValue::String(metrics_param.pod_name.clone()))
        }

        let mut buffer = String::new();
        let metrics: CgroupMeasurements = gather_value(&mut self.cgroup_v2_metric_file, &mut buffer)?;

        let diff_tot = match self.time_tot.update(metrics.cpu_time_total) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) | CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_usr = match self.time_usr.update(metrics.cpu_time_user_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_sys = match self.time_sys.update(metrics.cpu_time_system_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };

        // Push cpu total usage measure for user and system
        if let Some(value_tot) = diff_tot {
            let p_tot = create_measurement_point(
                timestamp,
                self.cpu_time_tot,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_tot,
                &metrics,
            );
            measurements.push(p_tot);
        }

        // Push cpu usage measure for user
        if let Some(value_usr) = diff_usr {
            let p_usr = create_measurement_point(
                timestamp,
                self.cpu_time_user_mode,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_usr,
                &metrics,
            );
            measurements.push(p_usr);
        }

        // Push cpu usage measure for system
        if let Some(value_sys) = diff_sys {
            let p_sys = create_measurement_point(
                timestamp,
                self.cpu_time_system_mode,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_sys,
                &metrics,
            );
            measurements.push(p_sys);
        }

        // Push anonymous used memory measure corresponding to running process and various allocated memory
        let mem_anon_value = metrics.memory_anonymous;
        let m_anon = create_measurement_point(
            timestamp,
            self.memory_anon,
            self.cgroup_v2_metric_file.consumer_memory.clone(),
            mem_anon_value,
            &metrics,
        );
        measurements.push(m_anon);

        // Push files memory measure, corresponding to open files and descriptors
        let mem_file_value = metrics.memory_file;
        let m_file = create_measurement_point(
            timestamp,
            self.memory_file,
            self.cgroup_v2_metric_file.consumer_memory.clone(),
            mem_file_value,
            &metrics,
        );
        measurements.push(m_file);

        // Push kernel memory measure
        let mem_kernel_value = metrics.memory_kernel;
        let m_ker = create_measurement_point(
            timestamp,
            self.memory_kernel,
            self.cgroup_v2_metric_file.consumer_memory.clone(),
            mem_kernel_value,
            &metrics,
        );
        measurements.push(m_ker);

        // Push pagetables memory measure
        let mem_pagetables_value = metrics.memory_pagetables;
        let m_pgt = create_measurement_point(
            timestamp,
            self.memory_pagetables,
            self.cgroup_v2_metric_file.consumer_memory.clone(),
            mem_pagetables_value,
            &metrics,
        );
        measurements.push(m_pgt);

        // Push total memory used by cgroup measure
        let mem_total_value = mem_anon_value + mem_file_value + mem_kernel_value + mem_pagetables_value;
        let m_tot = create_measurement_point(
            timestamp,
            self.memory_total,
            self.cgroup_v2_metric_file.consumer_memory.clone(),
            mem_total_value,
            &metrics,
        );
        measurements.push(m_tot);

        Ok(())
    }
}
