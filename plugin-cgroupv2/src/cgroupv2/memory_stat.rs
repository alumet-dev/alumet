use alumet::{
    measurement::{AttributeValue, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::source::PollError,
    resources::ResourceConsumer,
};
use anyhow::Context;

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek};

use crate::cgroupv2::{add_additional_attrs, measurement_to_point};

/// MemoryStatAlumetProbe is a high level component that manage the cgroup memory.stat measurements collection and adapt it to Alumet interfaces.
pub struct MemoryStatAlumetProbe {
    collector: MemoryStatCollector,
    consumer: ResourceConsumer,

    anon_metric: Option<TypedMetricId<u64>>,
    anon_additional_attrs: Option<Vec<(String, AttributeValue)>>,

    file_metric: Option<TypedMetricId<u64>>,
    file_additional_attrs: Option<Vec<(String, AttributeValue)>>,

    kernel_stack_metric: Option<TypedMetricId<u64>>,
    kernel_stack_additional_attrs: Option<Vec<(String, AttributeValue)>>,

    pagetables_metric: Option<TypedMetricId<u64>>,
    pagetables_additional_attrs: Option<Vec<(String, AttributeValue)>>,
    // could be extended to manage other memory.stat measurements
}

struct MemoryStatCollector {
    file: File,
    buffer: String,
    collected_keys: HashSet<String>,
    collect_anon: bool,
    collect_file: bool,
    collect_kernel_stack: bool,
    collect_pagetables: bool,
    // could be extended to manage other memory.stat measurements
}

struct MemoryStats {
    anon: Option<u64>,
    file: Option<u64>,
    kernel_stack: Option<u64>,
    pagetables: Option<u64>,
    // could be extended to manage other memory.stat measurements
}

impl MemoryStatAlumetProbe {
    /// new is a factory to create a MemoryStatAlumetProbe component:
    /// The filepath parameter should be a memory.stat file.
    /// The collect parameters allow to enable or disable the collection of memory.stat measurements.
    /// The metric parameters (TypedMetricId) allow to map a memory.stat measurement to an Alumet metric.
    /// The additional attributes parameters allow to extend specific attributes set to an Alumet metric.
    pub fn new(
        filepath: String,
        collect_anon: bool,
        collect_file: bool,
        collect_kernel_stack: bool,
        collect_pagetables: bool,
        anon_metric: Option<TypedMetricId<u64>>,
        file_metric: Option<TypedMetricId<u64>>,
        kernel_stack_metric: Option<TypedMetricId<u64>>,
        pagetables_metric: Option<TypedMetricId<u64>>,
        anon_additional_attrs: Option<Vec<(String, AttributeValue)>>,
        file_additional_attrs: Option<Vec<(String, AttributeValue)>>,
        kernel_stack_additional_attrs: Option<Vec<(String, AttributeValue)>>,
        pagetables_additional_attrs: Option<Vec<(String, AttributeValue)>>,
    ) -> Result<Self, anyhow::Error> {
        Self::ensure_metrics(
            collect_anon,
            collect_file,
            collect_kernel_stack,
            collect_pagetables,
            anon_metric.is_some(),
            file_metric.is_some(),
            kernel_stack_metric.is_some(),
            pagetables_metric.is_some(),
        )?;
        Ok(Self {
            collector: MemoryStatCollector::new(
                File::open(filepath.clone())?,
                collect_anon,
                collect_file,
                collect_kernel_stack,
                collect_pagetables,
            ),
            consumer: ResourceConsumer::ControlGroup {
                path: filepath.clone().into(),
            },
            anon_metric,
            anon_additional_attrs,
            file_metric,
            file_additional_attrs,
            kernel_stack_metric,
            kernel_stack_additional_attrs,
            pagetables_metric,
            pagetables_additional_attrs,
        })
    }

    pub fn add_anon_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.anon_additional_attrs, attributes);
    }

    pub fn add_file_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.file_additional_attrs, attributes);
    }

    pub fn add_kernel_stack_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.kernel_stack_additional_attrs, attributes);
    }

    pub fn add_pagetables_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.pagetables_additional_attrs, attributes);
    }

    pub fn add_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        self.add_anon_additional_attrs(attributes.clone());
        self.add_file_additional_attrs(attributes.clone());
        self.add_kernel_stack_additional_attrs(attributes.clone());
        self.add_pagetables_additional_attrs(attributes.clone());
    }

    pub fn collect_measurements(&mut self, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
        let mut measurement_points = Vec::<MeasurementPoint>::new();

        let memory_stats = self.collector.read_measurements()?;

        if let Some(anon) = memory_stats.anon {
            measurement_points.push(measurement_to_point(
                timestamp,
                self.anon_metric.unwrap(),
                self.consumer.clone(),
                anon,
                self.anon_additional_attrs.clone(),
            ));
        };
        if let Some(file) = memory_stats.file {
            measurement_points.push(measurement_to_point(
                timestamp,
                self.file_metric.unwrap(),
                self.consumer.clone(),
                file,
                self.file_additional_attrs.clone(),
            ));
        };
        if let Some(kernel_stack) = memory_stats.kernel_stack {
            measurement_points.push(measurement_to_point(
                timestamp,
                self.kernel_stack_metric.unwrap(),
                self.consumer.clone(),
                kernel_stack,
                self.kernel_stack_additional_attrs.clone(),
            ));
        };
        if let Some(pagetables) = memory_stats.pagetables {
            measurement_points.push(measurement_to_point(
                timestamp,
                self.pagetables_metric.unwrap(),
                self.consumer.clone(),
                pagetables,
                self.pagetables_additional_attrs.clone(),
            ));
        };

        Ok(measurement_points)
    }

    fn ensure_metrics(
        collect_anon: bool,
        collect_file: bool,
        collect_kernel_stack: bool,
        collect_pagetables: bool,
        anon_metric: bool,
        file_metric: bool,
        kernel_stack_metric: bool,
        pagetables_metric: bool,
    ) -> Result<(), anyhow::Error> {
        if collect_anon && !anon_metric {
            return Err(anyhow::anyhow!(
                "anon_metric must be provided when collect_anon is true"
            ));
        }
        if collect_file && !file_metric {
            return Err(anyhow::anyhow!(
                "file_metric must be provided when collect_file is true"
            ));
        }
        if collect_kernel_stack && !kernel_stack_metric {
            return Err(anyhow::anyhow!(
                "kernel_stack_metric must be provided when collect_kernel_stack is true"
            ));
        }
        if collect_pagetables && !pagetables_metric {
            return Err(anyhow::anyhow!(
                "pagetables_metric must be provided when collect_pagetables is true"
            ));
        }
        Ok(())
    }
}

impl MemoryStatCollector {
    fn new(
        file: File,
        collect_anon: bool,
        collect_file: bool,
        collect_kernel_stack: bool,
        collect_pagetables: bool,
    ) -> Self {
        let mut collector = Self {
            file,
            buffer: String::new(),
            collected_keys: HashSet::new(),
            collect_anon,
            collect_file,
            collect_kernel_stack,
            collect_pagetables,
        };
        collector.reload_collected_keys();
        collector
    }

    fn read_measurements(&mut self) -> Result<MemoryStats, anyhow::Error> {
        self.file.rewind()?;
        self.buffer.clear();
        self.file.read_to_string(&mut self.buffer)?;

        let mut memory_stats = MemoryStats::empty();

        for line in self.buffer.lines() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let key = parts[0];
            if !self.collected_keys.contains(key) {
                continue;
            }

            let value = parts[1]
                .parse::<u64>()
                .with_context(|| format!("Parsing of value: {}", parts[1]))?;

            match key {
                "anon" => memory_stats.anon = Some(value),
                "file" => memory_stats.file = Some(value),
                "kernel_stack" => memory_stats.kernel_stack = Some(value),
                "pagetables" => memory_stats.pagetables = Some(value),
                _ => {}
            }
        }

        Ok(memory_stats)
    }

    fn reload_collected_keys(&mut self) {
        self.collected_keys = HashSet::new();

        if self.collect_anon {
            self.collected_keys.insert("anon".to_string());
        }
        if self.collect_file {
            self.collected_keys.insert("file".to_string());
        }
        if self.collect_kernel_stack {
            self.collected_keys.insert("kernel_stack".to_string());
        }
        if self.collect_pagetables {
            self.collected_keys.insert("pagetables".to_string());
        }
    }
}

impl MemoryStats {
    fn empty() -> Self {
        Self {
            anon: None,
            file: None,
            kernel_stack: None,
            pagetables: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroupv2::tests_mock::{MemoryStatMock, MockFileCgroupKV};

    #[test]
    fn test_memory_stats_empty() {
        let empty = MemoryStats::empty();
        assert_eq!(empty.anon, None);
        assert_eq!(empty.file, None);
        assert_eq!(empty.kernel_stack, None);
        assert_eq!(empty.pagetables, None);
    }

    #[test]
    fn test_memory_stat_collector() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = MemoryStatMock::default();
        mock.anon = 63;
        mock.file = 12;
        mock.kernel_stack = 123;
        mock.pagetables = 42;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, true, true, true, true);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, Some(63));
        assert_eq!(memory_stats.file, Some(12));
        assert_eq!(memory_stats.kernel_stack, Some(123));
        assert_eq!(memory_stats.pagetables, Some(42));

        Ok(())
    }

    #[test]
    fn test_memory_stat_collector_no_anon() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = MemoryStatMock::default();
        mock.anon = 63;
        mock.file = 12;
        mock.kernel_stack = 123;
        mock.pagetables = 42;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, false, true, true, true);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, None);
        assert_eq!(memory_stats.file, Some(12));
        assert_eq!(memory_stats.kernel_stack, Some(123));
        assert_eq!(memory_stats.pagetables, Some(42));

        Ok(())
    }

    #[test]
    fn test_memory_stat_collector_no_file() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = MemoryStatMock::default();
        mock.anon = 63;
        mock.file = 12;
        mock.kernel_stack = 123;
        mock.pagetables = 42;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, true, false, true, true);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, Some(63));
        assert_eq!(memory_stats.file, None);
        assert_eq!(memory_stats.kernel_stack, Some(123));
        assert_eq!(memory_stats.pagetables, Some(42));

        Ok(())
    }

    #[test]
    fn test_memory_stat_collector_no_kernel_stack() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = MemoryStatMock::default();
        mock.anon = 63;
        mock.file = 12;
        mock.kernel_stack = 123;
        mock.pagetables = 42;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, true, true, false, true);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, Some(63));
        assert_eq!(memory_stats.file, Some(12));
        assert_eq!(memory_stats.kernel_stack, None);
        assert_eq!(memory_stats.pagetables, Some(42));

        Ok(())
    }

    #[test]
    fn test_memory_stat_collector_no_pagetables() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = MemoryStatMock::default();
        mock.anon = 63;
        mock.file = 12;
        mock.kernel_stack = 123;
        mock.pagetables = 42;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, true, true, true, false);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, Some(63));
        assert_eq!(memory_stats.file, Some(12));
        assert_eq!(memory_stats.kernel_stack, Some(123));
        assert_eq!(memory_stats.pagetables, None);

        Ok(())
    }

    #[test]
    fn test_memory_stat_collector_empty_file() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();
        let _ = File::create(file_path)?;

        let mut collector = MemoryStatCollector::new(File::open(file_path)?, true, true, true, true);
        let memory_stats = collector.read_measurements()?;

        assert_eq!(memory_stats.anon, None);
        assert_eq!(memory_stats.file, None);
        assert_eq!(memory_stats.kernel_stack, None);
        assert_eq!(memory_stats.pagetables, None);

        Ok(())
    }
}
