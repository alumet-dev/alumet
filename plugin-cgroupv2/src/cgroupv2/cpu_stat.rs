use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    plugin::util::CounterDiff,
    resources::ResourceConsumer,
};
use anyhow::{Context, Result};

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek};

use crate::cgroupv2::{add_additional_attrs, measurement_to_point, MeasurementAlumetMapping};
pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

/// CpuStatAlumetProbe is a high level component that manage the cgroup cpu.stat measurements collection and adapt it to Alumet interfaces.
pub struct CpuStatAlumetProbe {
    collector: CpuStatCollector,
    consumer: ResourceConsumer,

    usage_metric: Option<TypedMetricId<u64>>,
    usage_counter_diff: Option<CounterDiff>,
    usage_additional_attrs: Option<Vec<(String, AttributeValue)>>,

    user_metric: Option<TypedMetricId<u64>>,
    user_counter_diff: Option<CounterDiff>,
    user_additional_attrs: Option<Vec<(String, AttributeValue)>>,

    system_metric: Option<TypedMetricId<u64>>,
    system_counter_diff: Option<CounterDiff>,
    system_additional_attrs: Option<Vec<(String, AttributeValue)>>,
    // could be extended to manage other cpu.stat measurements
}

/// CpuStatCollector manage the collection of cpu.stat related measurements
struct CpuStatCollector {
    file: File,
    buffer: String,
    collected_line_indices: HashSet<usize>,
    collect_usage: bool,
    collect_user: bool,
    collect_system: bool,
    // could be extended to manage other cpu.stat measurements
}

/// CpuStats represents the cpu.stat file measurements
struct CpuStats {
    usage: Option<u64>,
    user: Option<u64>,
    system: Option<u64>,
    // could be extended to manage other cpu.stat measurements
}

impl CpuStatAlumetProbe {
    /// new is a factory to create a CpuStatAlumetProbe component:
    /// The filepath parameter should be a cpu.stat file.
    /// The MeasurementAlumetMapping parameters allow to map a measure from the CpuStatCollector to an Alumet metric
    /// In case it's None the measurement will not be collected
    pub fn new(
        filepath: String,
        usage_mapping: Option<MeasurementAlumetMapping>,
        user_mapping: Option<MeasurementAlumetMapping>,
        system_mapping: Option<MeasurementAlumetMapping>,
    ) -> Result<Self, anyhow::Error> {
        let collect_usage = usage_mapping.is_some();
        let collect_user = user_mapping.is_some();
        let collect_system = system_mapping.is_some();

        let (usage_metric, usage_counter_diff, usage_additional_attrs) = if collect_usage {
            let usage_mapping = usage_mapping.unwrap();
            (
                Some(usage_mapping.metric),
                Some(CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER)),
                usage_mapping.additional_attrs,
            )
        } else {
            (None, None, None)
        };

        let (user_metric, user_counter_diff, user_additional_attrs) = if collect_user {
            let user_mapping = user_mapping.unwrap();
            (
                Some(user_mapping.metric),
                Some(CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER)),
                user_mapping.additional_attrs,
            )
        } else {
            (None, None, None)
        };

        let (system_metric, system_counter_diff, system_additional_attrs) = if collect_system {
            let system_mapping = system_mapping.unwrap();
            (
                Some(system_mapping.metric),
                Some(CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER)),
                system_mapping.additional_attrs,
            )
        } else {
            (None, None, None)
        };

        Ok(Self {
            collector: CpuStatCollector::new(
                File::open(filepath.clone())?,
                collect_usage,
                collect_user,
                collect_system,
            )?,
            consumer: ResourceConsumer::ControlGroup {
                path: filepath.clone().into(),
            },
            usage_metric,
            usage_counter_diff,
            usage_additional_attrs,
            user_metric,
            user_counter_diff,
            user_additional_attrs,
            system_metric,
            system_counter_diff,
            system_additional_attrs,
        })
    }

    pub fn add_usage_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.usage_additional_attrs, attributes);
    }

    pub fn add_user_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.user_additional_attrs, attributes);
    }

    pub fn add_system_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        add_additional_attrs(&mut self.system_additional_attrs, attributes);
    }

    pub fn add_additional_attrs(&mut self, attributes: Vec<(String, AttributeValue)>) {
        self.add_usage_additional_attrs(attributes.clone());
        self.add_user_additional_attrs(attributes.clone());
        self.add_system_additional_attrs(attributes.clone());
    }

    pub fn collect_measurements(
        &mut self,
        timestamp: Timestamp,
        measurements: &mut MeasurementAccumulator,
    ) -> Result<(), anyhow::Error> {
        let mut push_measurement_to_alumet = |timestamp: Timestamp,
                                              counter_diff: &mut CounterDiff,
                                              metric: TypedMetricId<u64>,
                                              consumer: ResourceConsumer,
                                              value: u64,
                                              additional_attrs: Option<Vec<(String, AttributeValue)>>|
         -> Result<(), anyhow::Error> {
            let diff = counter_diff.update(value).difference();
            if let Some(diff) = diff {
                measurements.push(measurement_to_point(
                    timestamp,
                    metric,
                    consumer,
                    diff,
                    additional_attrs,
                ));
            }
            Ok(())
        };

        let cpu_stats = self.collector.read_measurements()?;

        if let Some(usage) = cpu_stats.usage {
            push_measurement_to_alumet(
                timestamp,
                self.usage_counter_diff.as_mut().unwrap(),
                self.usage_metric.unwrap(),
                self.consumer.clone(),
                usage,
                self.usage_additional_attrs.clone(),
            )?;
        }
        if let Some(user) = cpu_stats.user {
            push_measurement_to_alumet(
                timestamp,
                self.user_counter_diff.as_mut().unwrap(),
                self.user_metric.unwrap(),
                self.consumer.clone(),
                user,
                self.user_additional_attrs.clone(),
            )?;
        }
        if let Some(system) = cpu_stats.system {
            push_measurement_to_alumet(
                timestamp,
                self.system_counter_diff.as_mut().unwrap(),
                self.system_metric.unwrap(),
                self.consumer.clone(),
                system,
                self.system_additional_attrs.clone(),
            )?;
        }

        Ok(())
    }
}

impl CpuStatCollector {
    fn new(file: File, collect_usage: bool, collect_user: bool, collect_system: bool) -> Result<Self, anyhow::Error> {
        let mut collector = Self {
            file,
            buffer: String::new(),
            collected_line_indices: HashSet::new(),
            collect_usage,
            collect_user,
            collect_system,
        };
        collector.reload_collected_line_indices()?;
        Ok(collector)
    }

    fn read_measurements(&mut self) -> Result<CpuStats, anyhow::Error> {
        self.file.rewind()?;
        self.buffer.clear();
        self.file.read_to_string(&mut self.buffer)?;

        let mut cpu_stats = CpuStats::empty();

        for (i, line) in self.buffer.lines().enumerate() {
            if self.collected_line_indices.contains(&i) {
                let parts: Vec<&str> = line.split_ascii_whitespace().collect();
                if parts.len() < 2 {
                    continue;
                }
                let value = parts[1]
                    .parse::<u64>()
                    .with_context(|| format!("Parsing of value: {}", parts[1]))?;

                match parts[0] {
                    "usage_usec" => cpu_stats.usage = Some(value),
                    "user_usec" => cpu_stats.user = Some(value),
                    "system_usec" => cpu_stats.system = Some(value),
                    _ => continue,
                }
            }
        }
        Ok(cpu_stats)
    }

    fn reload_collected_line_indices(&mut self) -> std::io::Result<()> {
        self.file.rewind()?;
        self.buffer.clear();
        self.file.read_to_string(&mut self.buffer)?;

        self.collected_line_indices = HashSet::new();

        for (i, line) in self.buffer.lines().enumerate() {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let key = parts[0];
            match key {
                "usage_usec" => {
                    if self.collect_usage {
                        self.collected_line_indices.insert(i);
                    }
                }
                "user_usec" => {
                    if self.collect_user {
                        self.collected_line_indices.insert(i);
                    }
                }
                "system_usec" => {
                    if self.collect_system {
                        self.collected_line_indices.insert(i);
                    }
                }
                _ => {}
            }
        }

        Ok(())
    }
}

impl CpuStats {
    fn empty() -> Self {
        Self {
            usage: None,
            user: None,
            system: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgroupv2::tests_mock::{CpuStatMock, MockFileCgroupKV};

    #[test]
    fn test_cpu_stats_empty() {
        let empty = CpuStats::empty();
        assert_eq!(empty.usage, None);
        assert_eq!(empty.user, None);
        assert_eq!(empty.system, None);
    }

    #[test]
    fn test_cpu_stat_collector() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = CpuStatMock::default();
        mock.usage_usec = 63;
        mock.user_usec = 12;
        mock.system_usec = 123;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = CpuStatCollector::new(File::open(file_path)?, true, true, true)?;
        let cpu_stats = collector.read_measurements()?;

        assert_eq!(cpu_stats.usage, Some(63));
        assert_eq!(cpu_stats.user, Some(12));
        assert_eq!(cpu_stats.system, Some(123));

        Ok(())
    }

    #[test]
    fn test_cpu_stat_collector_no_usage() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = CpuStatMock::default();
        mock.usage_usec = 63;
        mock.user_usec = 12;
        mock.system_usec = 123;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = CpuStatCollector::new(File::open(file_path)?, false, true, true)?;
        let cpu_stats = collector.read_measurements()?;

        assert_eq!(cpu_stats.usage, None);
        assert_eq!(cpu_stats.user, Some(12));
        assert_eq!(cpu_stats.system, Some(123));

        Ok(())
    }

    #[test]
    fn test_cpu_stat_collector_no_user() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = CpuStatMock::default();
        mock.usage_usec = 63;
        mock.user_usec = 12;
        mock.system_usec = 123;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = CpuStatCollector::new(File::open(file_path)?, true, false, true)?;
        let cpu_stats = collector.read_measurements()?;

        assert_eq!(cpu_stats.usage, Some(63));
        assert_eq!(cpu_stats.user, None);
        assert_eq!(cpu_stats.system, Some(123));

        Ok(())
    }

    #[test]
    fn test_cpu_stat_collector_no_system() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = CpuStatMock::default();
        mock.usage_usec = 63;
        mock.user_usec = 12;
        mock.system_usec = 123;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let mut collector = CpuStatCollector::new(File::open(file_path)?, true, true, false)?;
        let cpu_stats = collector.read_measurements()?;

        assert_eq!(cpu_stats.usage, Some(63));
        assert_eq!(cpu_stats.user, Some(12));
        assert_eq!(cpu_stats.system, None);

        Ok(())
    }

    #[test]
    fn test_cpu_stat_collector_empty_file() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();
        let _ = File::create(file_path)?;

        let mut collector = CpuStatCollector::new(File::open(file_path)?, true, true, true)?;
        let cpu_stats = collector.read_measurements()?;

        assert_eq!(cpu_stats.usage, None);
        assert_eq!(cpu_stats.user, None);
        assert_eq!(cpu_stats.system, None);

        Ok(())
    }

    #[test]
    fn test_reload_collected_line_indices() -> Result<(), anyhow::Error> {
        let temp_file = tempfile::NamedTempFile::new()?;
        let file_path = temp_file.path();

        let mut mock = CpuStatMock::default();
        mock.usage_usec = 63;
        mock.user_usec = 12;
        mock.system_usec = 123;

        let file = File::create(file_path)?;
        mock.write_to_file(file)?;

        let collector = CpuStatCollector::new(File::open(file_path)?, true, false, true)?;
        let mut expected_line_indices = HashSet::new();
        expected_line_indices.insert(0);
        expected_line_indices.insert(2);
        assert_eq!(collector.collected_line_indices, expected_line_indices);
        Ok(())
    }
}
