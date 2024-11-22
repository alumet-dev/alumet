//! # Probe file for k8s module
//!
//! This module provides functionality to publish structured data for alumet,
//! by pushing CPU and memory cgroup Kubernetes informations on Unix-based systems.
use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use super::utils::{gather_value, CgroupV2MetricFile};
use crate::cgroupv2::{CgroupV2Metric, Metrics};

pub struct K8SProbe {
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

    /// Automatically create a measurement point to push for a pod, with pre-implemented settings :
    /// - `uid` of the pod
    /// - `name` of the pod
    /// - `namespace` of the pod
    /// - `node` of the pod
    ///
    /// # Parameters
    ///
    /// - `timestamp` : Type Timestamp
    /// - `metric_id` : TypedMetricId<u64>
    /// - `resource_consumer` : Type ResourceConsumer
    /// - `value_measured` : Type u64
    /// - `metrics_param` : Type CgroupV2Metric
    ///
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
        .with_attr("uid", AttributeValue::String(metrics_param.uid.clone()))
        .with_attr("name", AttributeValue::String(metrics_param.name.clone()))
        .with_attr("namespace", AttributeValue::String(metrics_param.namespace.clone()))
        .with_attr("node", AttributeValue::String(metrics_param.node.clone()))
    }
}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut file_buffer: String = String::new();
        let metrics: CgroupV2Metric = gather_value(&mut self.cgroup_v2_metric_file, &mut file_buffer)?;

        let diff_tot: Option<u64> = self.time_tot.update(metrics.time_used_tot).difference();
        let diff_usr: Option<u64> = self.time_usr.update(metrics.time_used_user_mode).difference();
        let diff_sys: Option<u64> = self.time_sys.update(metrics.time_used_system_mode).difference();

        // CPU ressource consumer for cpu.stat file in cgroup
        let consumer_cpu: ResourceConsumer = ResourceConsumer::ControlGroup {
            path: (self.cgroup_v2_metric_file.path_cpu.to_string_lossy().to_string().into()),
        };

        // Memory ressource consumer for memory.stat file in cgroup
        let consumer_memory: ResourceConsumer = ResourceConsumer::ControlGroup {
            path: (self
                .cgroup_v2_metric_file
                .path_memory
                .to_string_lossy()
                .to_string()
                .into()),
        };

        // Push cpu total usage measure for user and system
        if let Some(value_tot) = diff_tot {
            let p_tot: MeasurementPoint =
                self.create_measurement_point(timestamp, self.time_used_tot, consumer_cpu.clone(), value_tot, &metrics);
            measurements.push(p_tot);
        }

        // Push cpu usage measure for user
        if let Some(value_usr) = diff_usr {
            let p_usr: MeasurementPoint = self.create_measurement_point(
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
            let p_sys: MeasurementPoint = self.create_measurement_point(
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
        let m_anon: MeasurementPoint = self.create_measurement_point(
            timestamp,
            self.anon_used_mem,
            consumer_memory.clone(),
            mem_anon_value / 1000,
            &metrics,
        );
        measurements.push(m_anon);

        // Push files memory measure, corresponding to open files and descriptors
        let mem_file_value = metrics.file_mem;
        let m_file: MeasurementPoint = self.create_measurement_point(
            timestamp,
            self.file_mem,
            consumer_memory.clone(),
            mem_file_value / 1000,
            &metrics,
        );
        measurements.push(m_file);

        // Push kernel memory measure
        let mem_kernel_value = metrics.kernel_mem;
        let m_ker: MeasurementPoint = self.create_measurement_point(
            timestamp,
            self.kernel_mem,
            consumer_memory.clone(),
            mem_kernel_value / 1000,
            &metrics,
        );
        measurements.push(m_ker);

        // Push pagetables memory measure
        let mem_pagetables_value = metrics.pagetables_mem;
        let m_pgt: MeasurementPoint = self.create_measurement_point(
            timestamp,
            self.pagetables_mem,
            consumer_memory.clone(),
            mem_pagetables_value / 1000,
            &metrics,
        );
        measurements.push(m_pgt);

        // Push total memory used by cgroup measure
        let mem_total_value = (mem_anon_value + mem_file_value + mem_kernel_value + mem_pagetables_value) / 1000;
        let m_tot: MeasurementPoint = self.create_measurement_point(
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

// ------------------ //
// --- UNIT TESTS --- //
// ------------------ //
/*#[cfg(test)]
mod tests {
    use super::*;
    use alumet::measurement::MeasurementAccumulator;
    use alumet::metrics::MetricTypeError;
    use alumet::pipeline::Source;
    use alumet::pipeline::elements::error::PollError;
    use std::fs::File;
    use std::path::PathBuf;

    fn create_fake_metrics(usec: &str) -> Result<Metrics, MetricTypeError> {
        let time_used_tot: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_cpu_usage_total",
            usec.to_string(),
            "Total CPU usage time by the cgroup",
        )?;
        
        let time_used_user_mode: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_cpu_usage_user",
            usec.to_string(),
            "CPU in user mode usage time by the cgroup",
        )?;

        let time_used_system_mode: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_cpu_usage_system",
            usec.to_string(),
            "CPU in system mode usage time by the cgroup",
        )?;

        let anon_used_mem: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_memory_anon",
            usec.to_string(),
            "Anonymous used memory by the cgroup",
        )?;

        let file_mem: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_memory_file",
            usec.to_string(),
            "Files memory used by the cgroup",
        )?;

        let kernel_mem: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_memory_kernel",
            usec.to_string(),
            "Kernel memory used by the cgroup",
        )?;

        let pagetables_mem: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_memory_pagetables",
            usec.to_string(),
            "Memory used for page tables by the cgroup",
        )?;

        let total_mem: TypedMetricId<u64> = alumet.create_metric::<u64>(
            "cgroup_memory_total",
            usec.to_string(),
            "Total memory used by the cgroup",
        )?;

        Ok(Metrics {
            time_used_tot,
            time_used_user_mode,
            time_used_system_mode,
            anon_used_mem,
            file_mem,
            kernel_mem,
            pagetables_mem,
            total_mem,
        })
    }

    #[test]
    fn test_k8s_probe_poll() -> Result<(), PollError> {
        let usec = "some_unique_identifier";
        let metrics = create_fake_metrics(usec)?;

        let path_cpu: PathBuf = "/path/to/cpu.stat".into();
        let path_memory: PathBuf = "/path/to/memory.stat".into();

        // CPU stat file
        let file_cpu = File::open(&path_cpu).expect("couldn't open cpu.stat file");
        
        // Memory stat file
        let file_memory = File::open(&path_memory).expect("couldn't open memory.stat file");

        let metric_file = CgroupV2MetricFile {
            path_cpu,
            path_memory,
            file_cpu,
            file_memory,
            name: "metric_name".to_string(),
            uid: "uid_test".to_string(),
            namespace: "namespace_test".to_string(),
            node: "node_test".to_string(),
        };

        let counter_tot = CounterDiff::with_max_value(0);
        let counter_usr = CounterDiff::with_max_value(0);
        let counter_sys = CounterDiff::with_max_value(0);

        let mut probe = K8SProbe::new(metrics, metric_file, counter_tot, counter_sys, counter_usr)?;
        let mut measurements = MeasurementAccumulator::new();

        let timestamp = Timestamp::now();
        probe.poll(&mut measurements, timestamp)?;

        assert!(!measurements.is_empty(), "The measuring accumulator must not be empty.");
        assert!(measurements.len() > 0, "There should be at least one measuring point.");

        Ok(())
    }
}*/
