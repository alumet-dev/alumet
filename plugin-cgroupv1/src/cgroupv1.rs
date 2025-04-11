use crate::oar2::probe::OarJobSource;
use alumet::{
    measurement::{MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
    pipeline::elements::error::PollError,
    plugin::AlumetPluginStart,
    resources::Resource,
    units::{PrefixedUnit, Unit},
};
use std::{
    io::{Read, Seek},
    result::Result::Ok,
};

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cpu_metric: TypedMetricId<u64>,
    pub memory_metric: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let nsec = PrefixedUnit::nano(Unit::Second);

        Ok(Self {
            cpu_metric: alumet.create_metric::<u64>(
                "cpu_time",
                nsec,
                "Total CPU time consumed by the cgroup (in nanoseconds).",
            )?,
            memory_metric: alumet.create_metric::<u64>(
                "memory_usage",
                Unit::Byte,
                "Total memory usage by the cgroup (in bytes).",
            )?,
        })
    }
}

pub fn gather_value(job_source: &mut OarJobSource, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
    let cpu_usage_file = &mut job_source.cgroup_v1_metric_file.cgroup_cpu_file;
    cpu_usage_file.rewind()?;
    let mut buffer = String::new();
    cpu_usage_file.read_to_string(&mut buffer)?;
    let cpu_usage_u64 = buffer.trim().parse::<u64>()?;
    buffer.clear();
    let memory_usage_file = &mut job_source.cgroup_v1_metric_file.cgroup_memory_file;
    memory_usage_file.rewind()?;
    memory_usage_file.read_to_string(&mut buffer)?;
    let memory_usage_u64 = buffer.trim().parse::<u64>()?;

    let mut measurement_point_vector: Vec<MeasurementPoint> = Vec::new();
    measurement_point_vector.push(
        MeasurementPoint::new(
            timestamp,
            job_source.memory_metric,
            Resource::LocalMachine,
            job_source.cgroup_v1_metric_file.memory_file_path.clone(),
            memory_usage_u64,
        )
        .with_attr("oar_job_id", job_source.cgroup_v1_metric_file.job_id.clone()),
    );
    measurement_point_vector.push(
        MeasurementPoint::new(
            timestamp,
            job_source.cpu_metric,
            Resource::LocalMachine,
            job_source.cgroup_v1_metric_file.cpu_file_path.clone(),
            cpu_usage_u64,
        )
        .with_attr("oar_job_id", job_source.cgroup_v1_metric_file.job_id.clone()),
    );
    Ok(measurement_point_vector)
}
