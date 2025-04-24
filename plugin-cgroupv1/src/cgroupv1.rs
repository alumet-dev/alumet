use alumet::{
    measurement::{AttributeValue, MeasurementPoint, Timestamp},
    metrics::{error::MetricCreationError, TypedMetricId},
    pipeline::elements::error::PollError,
    plugin::{
        util::{CounterDiff, CounterDiffUpdate},
        AlumetPluginStart,
    },
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::anyhow;
use std::{
    fs::File,
    io::{Read, Seek},
    result::Result::Ok,
};

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cpu_time_delta: TypedMetricId<u64>,
    pub memory_usage: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let nsec = PrefixedUnit::nano(Unit::Second);

        Ok(Self {
            cpu_time_delta: alumet.create_metric::<u64>(
                "cpu_time_delta",
                nsec,
                "Total CPU time consumed by the cgroup.",
            )?,
            memory_usage: alumet.create_metric::<u64>(
                "memory_usage",
                Unit::Byte,
                "Total memory usage by the cgroup.",
            )?,
        })
    }
}

pub struct Cgroupv1Probe {
    metrics: Metrics,

    cpu_time_delta_consumer: Option<ResourceConsumer>,
    cpu_time_delta_file: Option<File>,
    cpu_time_delta_counter_diff: Option<CounterDiff>,

    memory_usage_consumer: Option<ResourceConsumer>,
    memory_usage_file: Option<File>,
}

impl Cgroupv1Probe {
    pub fn new(
        metrics: Metrics,
        cpuacct_usage_filepath: Option<String>,
        memory_usage_in_bytes_filepath: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let mut probe = Self {
            metrics,
            cpu_time_delta_consumer: None,
            cpu_time_delta_file: None,
            cpu_time_delta_counter_diff: None,
            memory_usage_consumer: None,
            memory_usage_file: None,
        };
        if let Some(filepath) = cpuacct_usage_filepath {
            probe.cpu_time_delta_consumer = Some(ResourceConsumer::ControlGroup {
                path: filepath.clone().into(),
            });
            probe.cpu_time_delta_file = Some(File::open(filepath)?);
            probe.cpu_time_delta_counter_diff = Some(CounterDiff::with_max_value(u64::MAX));
        }
        if let Some(filepath) = memory_usage_in_bytes_filepath {
            probe.memory_usage_consumer = Some(ResourceConsumer::ControlGroup {
                path: filepath.clone().into(),
            });
            probe.memory_usage_file = Some(File::open(filepath)?);
        }
        Ok(probe)
    }

    pub fn collect_measurements(
        &mut self,
        timestamp: Timestamp,
        additional_attrs: &Vec<(String, AttributeValue)>,
    ) -> Result<Vec<MeasurementPoint>, PollError> {
        let mut measurement_points = Vec::<MeasurementPoint>::new();
        let mut buffer = String::new();

        if let Some(cpu_time_delta_file) = &mut self.cpu_time_delta_file {
            let cpu_time_total = collect_cpuacct_usage(&mut buffer, cpu_time_delta_file)?;
            let cpu_time_delta = match self
                .cpu_time_delta_counter_diff
                .as_mut()
                .ok_or(PollError::Fatal(anyhow!(
                    "cpu_time_delta_counter_diff shouldn't be None when cpu_time_delta_file is valid"
                )))?
                .update(cpu_time_total)
            {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            if let Some(cpu_time_delta_value) = cpu_time_delta {
                measurement_points.push(
                    MeasurementPoint::new(
                        timestamp,
                        self.metrics.cpu_time_delta,
                        Resource::LocalMachine,
                        self.cpu_time_delta_consumer.clone().ok_or(PollError::Fatal(anyhow!(
                            "cpu_time_delta_consumer shouldn't be None when cpu_time_delta_file is valid"
                        )))?,
                        cpu_time_delta_value,
                    )
                    .with_attr("kind", "total")
                    .with_attr_vec(additional_attrs.clone()),
                );
            }
        }

        if let Some(memory_usage_file) = &mut self.memory_usage_file {
            let memory_usage_u64 = collect_memory_usage(&mut buffer, memory_usage_file)?;
            measurement_points.push(
                MeasurementPoint::new(
                    timestamp,
                    self.metrics.memory_usage,
                    Resource::LocalMachine,
                    self.memory_usage_consumer.clone().unwrap(),
                    memory_usage_u64,
                )
                .with_attr("kind", "resident")
                .with_attr_vec(additional_attrs.clone()),
            );
        }

        Ok(measurement_points)
    }
}

fn collect_memory_usage(buffer: &mut String, file: &mut File) -> Result<u64, anyhow::Error> {
    collect_file_single_measurement(buffer, file)
}

fn collect_cpuacct_usage(buffer: &mut String, file: &mut File) -> Result<u64, anyhow::Error> {
    collect_file_single_measurement(buffer, file)
}

fn collect_file_single_measurement(buffer: &mut String, file: &mut File) -> Result<u64, anyhow::Error> {
    buffer.clear();
    file.rewind()?;
    file.read_to_string(buffer)?;
    let measurement = buffer.trim().parse::<u64>()?;
    Ok(measurement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_collect_memory_usage() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "123456").expect("Failed to write to temp file");
        let mut file = temp_file.reopen().expect("Failed to reopen temp file");
        let mut buffer = String::new();

        let result = collect_memory_usage(&mut buffer, &mut file).expect("Failed to collect memory usage");

        assert_eq!(result, 123456);
    }

    #[test]
    fn test_collect_cpuacct_usage() {
        let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
        writeln!(temp_file, "789012").expect("Failed to write to temp file");
        let mut file = temp_file.reopen().expect("Failed to reopen temp file");
        let mut buffer = String::new();

        let result = collect_cpuacct_usage(&mut buffer, &mut file).expect("Failed to collect CPU usage");

        assert_eq!(result, 789012);
    }
}
