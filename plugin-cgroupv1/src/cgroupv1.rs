use crate::oar2::probe::OAR2Probe;
use alumet::{
    measurement::{MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
    pipeline::elements::error::PollError,
    plugin::{util::CounterDiffUpdate, AlumetPluginStart},
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
                "cpu_time_delta",
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

pub fn gather_value(job_source: &mut OAR2Probe, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
    let cpu_usage_file = &mut job_source.oar2_metric_file.cgroup_cpu_file;
    cpu_usage_file.rewind()?;
    let mut buffer = String::new();
    cpu_usage_file.read_to_string(&mut buffer)?;
    let total_cpu_time = buffer.trim().parse::<u64>()?;
    buffer.clear();
    let memory_usage_file = &mut job_source.oar2_metric_file.cgroup_memory_file;
    memory_usage_file.rewind()?;
    memory_usage_file.read_to_string(&mut buffer)?;
    let memory_usage_u64 = buffer.trim().parse::<u64>()?;

    let mut measurement_point_vector: Vec<MeasurementPoint> = Vec::new();
    measurement_point_vector.push(
        MeasurementPoint::new(
            timestamp,
            job_source.memory_metric,
            Resource::LocalMachine,
            job_source.oar2_metric_file.memory_file_path.clone(),
            memory_usage_u64,
        )
        .with_attr("job_id", job_source.oar2_metric_file.job_id.clone()),
    );

    let cpu_time_delta = match job_source.cpu_metric_counter_diff.update(total_cpu_time) {
        CounterDiffUpdate::FirstTime => None,
        CounterDiffUpdate::Difference(diff) => Some(diff),
        CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
    };
    if let Some(cpu_time_delta_value) = cpu_time_delta {
        measurement_point_vector.push(
            MeasurementPoint::new(
                timestamp,
                job_source.cpu_metric,
                Resource::LocalMachine,
                job_source.oar2_metric_file.cpu_file_path.clone(),
                cpu_time_delta_value,
            )
            .with_attr("job_id", job_source.oar2_metric_file.job_id.clone()),
        );
    }

    Ok(measurement_point_vector)
}
