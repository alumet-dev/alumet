use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    resources::ResourceConsumer,
};

use std::fs::File;
use std::io::{Read, Seek};

use crate::cgroupv2::{add_additional_attrs, measurement_to_point, MeasurementAlumetMapping};

/// MemoryCurrentAlumetProbe is a high level component that manage the cgroup memory.current measurement collection and adapt it to Alumet interfaces.
pub struct MemoryCurrentAlumetProbe {
    collector: MemoryCurrentCollector,
    consumer: ResourceConsumer,

    metric: TypedMetricId<u64>,
    additional_attrs: Option<Vec<(String, AttributeValue)>>,
}

/// MemoryCurrentCollector manage the collection of memory.current related measurement
struct MemoryCurrentCollector {
    file: File,
    buffer: String,
}

impl MemoryCurrentAlumetProbe {
    /// new is a factory to create a MemoryCurrentAlumetProbe component:
    /// The filepath parameter should be a memory.current file.
    /// The metric parameter (TypedMetricId) allow to map the value of memory.current measurement to an Alumet metric.
    /// The additional attributes parameter allow to extend specific attributes set to the Alumet metric.
    pub fn new(filepath: String, config: MeasurementAlumetMapping) -> Result<Self, anyhow::Error> {
        Ok(Self {
            collector: MemoryCurrentCollector::new(File::open(filepath.clone())?),
            consumer: ResourceConsumer::ControlGroup {
                path: filepath.clone().into(),
            },
            metric: config.metric,
            additional_attrs: config.additional_attrs,
        })
    }

    pub fn add_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.additional_attrs, attributes);
    }

    pub fn collect_measurements(
        &mut self,
        timestamp: Timestamp,
        measurements: &mut MeasurementAccumulator,
    ) -> Result<(), anyhow::Error> {
        let current = self.collector.read_measurement()?;

        measurements.push(measurement_to_point(
            timestamp,
            self.metric,
            self.consumer.clone(),
            current,
            self.additional_attrs.clone(),
        ));

        Ok(())
    }
}

impl MemoryCurrentCollector {
    fn new(file: File) -> Self {
        Self {
            file,
            buffer: String::new(),
        }
    }
    fn read_measurement(&mut self) -> Result<u64, anyhow::Error> {
        self.file.rewind()?;
        self.buffer.clear();
        self.file.read_to_string(&mut self.buffer)?;

        self.buffer
            .trim()
            .parse::<u64>()
            .map_err(|e| anyhow::anyhow!("Failed to parse '{}': {}", self.buffer, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroupv2::tests_mock::MemoryCurrentMock;

    #[test]
    fn test_cpu_stat_collector() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mock = MemoryCurrentMock(42);

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryCurrentCollector::new(File::open(file_path)?);
        let current = collector.read_measurement()?;

        assert_eq!(current, 42);

        Ok(())
    }

    #[test]
    fn test_cpu_stat_collector_empty_file() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();
        let _ = File::create(file_path)?;

        let mut collector = MemoryCurrentCollector::new(File::open(file_path)?);
        let result = collector.read_measurement();

        match result {
            Ok(_) => panic!("Expected an error, but got Ok"),
            Err(e) => {
                assert_eq!(
                    e.to_string(),
                    "Failed to parse '': cannot parse integer from empty string"
                );
            }
        }

        Ok(())
    }
}
