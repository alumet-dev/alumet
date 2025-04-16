use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::error::{PollError, PollRetry},
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Context, Result};

use super::utils::{gather_value, CgroupV2MetricFile};
use crate::cgroupv2::{CgroupMeasurements, Metrics};

pub struct K8SProbe {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
    pub cpu_time_delta: TypedMetricId<u64>,
    pub memory_usage: TypedMetricId<u64>,
    pub memory_anon: TypedMetricId<u64>,
    pub memory_file: TypedMetricId<u64>,
    pub memory_kernel: TypedMetricId<u64>,
    pub memory_pagetables: TypedMetricId<u64>,
}

impl K8SProbe {
    pub fn new(
        metric: Metrics,
        metric_file: CgroupV2MetricFile,
        counter_tot: CounterDiff,
        counter_sys: CounterDiff,
        counter_usr: CounterDiff,
    ) -> anyhow::Result<K8SProbe> {
        Ok(K8SProbe {
            cgroup_v2_metric_file: metric_file,
            time_tot: counter_tot,
            time_usr: counter_usr,
            time_sys: counter_sys,
            cpu_time_delta: metric.cpu_time_delta,
            memory_usage: metric.memory_usage,
            memory_anon: metric.memory_anonymous,
            memory_file: metric.memory_file,
            memory_kernel: metric.memory_kernel,
            memory_pagetables: metric.memory_pagetables,
        })
    }
}

impl alumet::pipeline::Source for K8SProbe {
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
            .with_attr("uid", AttributeValue::String(metrics_param.pod_uid.clone()))
            .with_attr("name", AttributeValue::String(metrics_param.pod_name.clone()))
            .with_attr("namespace", AttributeValue::String(metrics_param.namespace.clone()))
            .with_attr("node", AttributeValue::String(metrics_param.node.clone()))
        }

        let mut buffer = String::new();
        let metrics = gather_value(&mut self.cgroup_v2_metric_file, &mut buffer)
            .context("Error get value")
            .retry_poll()?;

        let diff_tot = self.time_tot.update(metrics.cpu_time_total).difference();
        let diff_usr = self.time_usr.update(metrics.cpu_time_user_mode).difference();
        let diff_sys = self.time_sys.update(metrics.cpu_time_system_mode).difference();

        // Push cpu total usage measure for user and system
        if let Some(value_tot) = diff_tot {
            let p_tot = create_measurement_point(
                timestamp,
                self.cpu_time_delta,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_tot,
                &metrics,
            )
            .with_attr("kind", "total");
            measurements.push(p_tot);
        }

        // Push cpu usage measure for user
        if let Some(value_usr) = diff_usr {
            let p_usr = create_measurement_point(
                timestamp,
                self.cpu_time_delta,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_usr,
                &metrics,
            )
            .with_attr("kind", "user");
            measurements.push(p_usr);
        }

        // Push cpu usage measure for system
        if let Some(value_sys) = diff_sys {
            let p_sys = create_measurement_point(
                timestamp,
                self.cpu_time_delta,
                self.cgroup_v2_metric_file.consumer_cpu.clone(),
                value_sys,
                &metrics,
            )
            .with_attr("kind", "system");
            measurements.push(p_sys);
        }

        // Push resident memory usage corresponding to running process
        let mem_usage_value = metrics.memory_usage_resident;
        let m_usage_resident = create_measurement_point(
            timestamp,
            self.memory_usage,
            self.cgroup_v2_metric_file.consumer_memory_current.clone(),
            mem_usage_value,
            &metrics,
        )
        .with_attr("kind", "resident");
        measurements.push(m_usage_resident);

        // Push anonymous used memory measure corresponding to running process and various allocated memory
        let mem_anon_value = metrics.memory_anonymous;
        let m_anon = create_measurement_point(
            timestamp,
            self.memory_anon,
            self.cgroup_v2_metric_file.consumer_memory_stat.clone(),
            mem_anon_value,
            &metrics,
        );
        measurements.push(m_anon);

        // Push files memory measure, corresponding to open files and descriptors
        let mem_file_value = metrics.memory_file;
        let m_file = create_measurement_point(
            timestamp,
            self.memory_file,
            self.cgroup_v2_metric_file.consumer_memory_stat.clone(),
            mem_file_value,
            &metrics,
        );
        measurements.push(m_file);

        // Push kernel memory measure
        let mem_kernel_value = metrics.memory_kernel;
        let m_ker = create_measurement_point(
            timestamp,
            self.memory_kernel,
            self.cgroup_v2_metric_file.consumer_memory_stat.clone(),
            mem_kernel_value,
            &metrics,
        );
        measurements.push(m_ker);

        // Push pagetables memory measure
        let mem_pagetables_value = metrics.memory_pagetables;
        let m_pgt = create_measurement_point(
            timestamp,
            self.memory_pagetables,
            self.cgroup_v2_metric_file.consumer_memory_stat.clone(),
            mem_pagetables_value,
            &metrics,
        );
        measurements.push(m_pgt);

        Ok(())
    }
}
