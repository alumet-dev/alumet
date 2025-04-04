use alumet::measurement::Timestamp;
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
    resources::Resource,
};
use std::{
    io::{Read, Seek},
    result::Result::Ok,
};

use super::utils::Cgroupv1MetricFile;

use crate::cgroupv1::Metrics;

#[derive(Debug)]
pub struct OarJobSource {
    pub cpu_metric: TypedMetricId<u64>,
    pub memory_metric: TypedMetricId<u64>,
    pub cgroup_v1_metric_file: Cgroupv1MetricFile,
}

impl OarJobSource {
    pub fn new(metric: Metrics, metric_file: Cgroupv1MetricFile) -> anyhow::Result<OarJobSource> {
        Ok(OarJobSource {
            cpu_metric: metric.cpu_metric,
            memory_metric: metric.memory_metric,
            cgroup_v1_metric_file: metric_file,
        })
    }
}

impl Source for OarJobSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let cpu_usage_file = &mut self.cgroup_v1_metric_file.cgroup_cpu_file;
        cpu_usage_file.rewind()?;
        let mut cpu_usage = String::new();
        cpu_usage_file.read_to_string(&mut cpu_usage)?;

        let memory_usage_file = &mut self.cgroup_v1_metric_file.cgroup_memory_file;
        memory_usage_file.rewind()?;
        let mut memory_usage = String::new();
        memory_usage_file.read_to_string(&mut memory_usage)?;
        let cpu_usage_u64 = cpu_usage.trim().parse::<u64>()?;
        let memory_usage_u64 = memory_usage.trim().parse::<u64>()?;

        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.cpu_metric,
                Resource::LocalMachine,
                self.cgroup_v1_metric_file.cpu_file_path.clone(),
                cpu_usage_u64,
            )
            .with_attr("oar_job_id", self.cgroup_v1_metric_file.job_id.clone()),
        );

        measurements.push(
            MeasurementPoint::new(
                timestamp,
                self.memory_metric,
                Resource::LocalMachine,
                self.cgroup_v1_metric_file.memory_file_path.clone(),
                memory_usage_u64,
            )
            .with_attr("oar_job_id", self.cgroup_v1_metric_file.job_id.clone()),
        );

        Ok(())
    }
}
