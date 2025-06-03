// This file contains integration tests. Not having them in tests/ subdir is expected here.
// This is to bypass a rust behavior while cfg(test) annotations are not visible for integration tests under tests/ subdir
// In our context we want use these annotations to change how code is compiled under tests (most of time to mock constants)
#[cfg(test)]
use std::{
    fs::{self, File},
    io::Write,
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};

use tempfile::{tempdir, TempDir};

use crate::{Config, ProcessToCgroupBridgePlugin};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::naming::TransformName,
    pipeline::{elements::error::PollError, elements::source::trigger::TriggerSpec, Source},
    plugin::rust::AlumetPlugin,
    plugin::PluginMetadata,
    resources::{Resource, ResourceConsumer},
    test::{runtime::TransformCheckInputContext, RuntimeExpectations},
    units::Unit,
};
use lazy_static::lazy_static;

lazy_static! {
    static ref T: Timestamp = Timestamp::from(UNIX_EPOCH);
    static ref T2: Timestamp = Timestamp::from(UNIX_EPOCH + Duration::from_secs(1));
}

#[derive(Debug, Clone)]
struct ExpectedCounts {
    t_initial: usize,
    t2_initial: usize,
    t_shared: usize,
    t_single: usize,
    t2_shared: usize,
    t2_single: usize,
}

fn count_measurements_by_consumer_and_time(
    measurements: &MeasurementBuffer,
    consumer_filter: impl Fn(&ResourceConsumer) -> bool,
    timestamp: Timestamp,
) -> usize {
    measurements
        .iter()
        .filter(|p| consumer_filter(&p.consumer) && p.timestamp == timestamp)
        .count()
}

fn count_initial_measurements(measurements: &MeasurementBuffer, timestamp: Timestamp) -> usize {
    count_measurements_by_consumer_and_time(
        measurements,
        |consumer| {
            matches!(
                consumer,
                ResourceConsumer::Process { .. } | ResourceConsumer::LocalMachine
            )
        },
        timestamp,
    )
}

fn count_cgroup_measurements(measurements: &MeasurementBuffer, cgroup_path: &str, timestamp: Timestamp) -> usize {
    count_measurements_by_consumer_and_time(
        measurements,
        |consumer| matches!(consumer, ResourceConsumer::ControlGroup { path } if path == cgroup_path),
        timestamp,
    )
}

fn assert_measurement_counts(measurements: &MeasurementBuffer, expected: ExpectedCounts) {
    let t_initial_count = count_initial_measurements(measurements, *T);
    assert_eq!(t_initial_count, expected.t_initial);

    let t2_initial_count = count_initial_measurements(measurements, *T2);
    assert_eq!(t2_initial_count, expected.t2_initial);

    let t_shared_count = count_cgroup_measurements(measurements, "/system.slice/shared.slice", *T);
    assert_eq!(t_shared_count, expected.t_shared);

    let t_single_count = count_cgroup_measurements(measurements, "/system.slice/single.slice", *T);
    assert_eq!(t_single_count, expected.t_single);

    let t2_shared_count = count_cgroup_measurements(measurements, "/system.slice/shared.slice", *T2);
    assert_eq!(t2_shared_count, expected.t2_shared);

    let t2_single_count = count_cgroup_measurements(measurements, "/system.slice/single.slice", *T2);
    assert_eq!(t2_single_count, expected.t2_single);
}

fn run_test_with_config(config: Config, expected_counts: ExpectedCounts) -> anyhow::Result<()> {
    let _ = prepare_procfs_mock()?;
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbNvmlPlugin>(),
        enabled: true,
        config: None,
    });
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<ProcessToCgroupBridgePlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        prepare_mock_measurements(ctx).expect("failed to prepare mock points")
    };

    let check_output = move |measurements: &MeasurementBuffer| {
        assert_measurement_counts(measurements, expected_counts.clone());
    };

    let runtime_expectations = RuntimeExpectations::new().test_transform(
        TransformName::from_str("process-to-cgroup-bridge", "transform"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();

    Ok(())
}

#[test]
fn test_default_setup() -> anyhow::Result<()> {
    let tmp = prepare_procfs_mock()?;
    let base_path = tmp.path().to_path_buf();
    let config = Config {
        processes_metrics: string_vec(&["metric_a", "metric_b"]),
        proc_path: base_path,
        ..Default::default()
    };

    let expected = ExpectedCounts {
        t_initial: 6,
        t2_initial: 1,
        t_shared: 2,
        t_single: 1,
        t2_shared: 1,
        t2_single: 0,
    };

    run_test_with_config(config, expected)
}

#[test]
fn test_only_metric_a() -> anyhow::Result<()> {
    let tmp = prepare_procfs_mock()?;
    let base_path = tmp.path().to_path_buf();
    let config = Config {
        processes_metrics: string_vec(&["metric_a"]),
        proc_path: base_path,
        ..Default::default()
    };

    let expected = ExpectedCounts {
        t_initial: 6,
        t2_initial: 1,
        t_shared: 1,
        t_single: 1,
        t2_shared: 1,
        t2_single: 0,
    };

    run_test_with_config(config, expected)
}

#[test]
fn test_merge_config_disable() -> anyhow::Result<()> {
    let tmp = prepare_procfs_mock()?;
    let base_path = tmp.path().to_path_buf();
    let config = Config {
        processes_metrics: string_vec(&["metric_a", "metric_b"]),
        merge_similar_cgroups: false,
        proc_path: base_path,
        ..Default::default()
    };

    let expected = ExpectedCounts {
        t_initial: 6,
        t2_initial: 1,
        t_shared: 4,
        t_single: 1,
        t2_shared: 1,
        t2_single: 0,
    };

    run_test_with_config(config, expected)
}

#[test]
fn test_keep_config_disable() -> anyhow::Result<()> {
    let tmp = prepare_procfs_mock()?;
    let base_path = tmp.path().to_path_buf();
    let config = Config {
        processes_metrics: string_vec(&["metric_a", "metric_b"]),
        keep_processed_measurements: false,
        proc_path: base_path,
        ..Default::default()
    };

    let expected = ExpectedCounts {
        t_initial: 1,
        t2_initial: 0,
        t_shared: 2,
        t_single: 1,
        t2_shared: 1,
        t2_single: 0,
    };

    run_test_with_config(config, expected)
}

#[test]
fn test_process_id_not_found_in_procfs() -> anyhow::Result<()> {
    let tmp = prepare_procfs_mock()?;
    let base_path = tmp.path().to_path_buf();

    let mut plugins = PluginSet::new();

    let source_config = Config {
        processes_metrics: string_vec(&["metric_a", "metric_b"]),
        proc_path: base_path,
        ..Default::default()
    };

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbNvmlPlugin>(),
        enabled: true,
        config: None,
    });
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<ProcessToCgroupBridgePlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&source_config)),
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        let metric = ctx
            .metrics()
            .by_name("metric_a")
            .expect("metric_a metric should exist")
            .0;
        let mut m = MeasurementBuffer::new();
        let point = MeasurementPoint::new_untyped(
            *T,
            metric,
            Resource::LocalMachine,
            ResourceConsumer::Process { pid: 42 },
            WrappedMeasurementValue::U64(10),
        );
        m.push(point);
        m
    };

    let check_output = move |measurements: &MeasurementBuffer| {
        assert_eq!(measurements.len(), 1);
        assert_eq!(
            measurements.iter().next().unwrap().consumer,
            ResourceConsumer::Process { pid: 42 }
        );
    };

    let runtime_expectations = RuntimeExpectations::new().test_transform(
        TransformName::from_str("process-to-cgroup-bridge", "transform"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();

    Ok(())
}

fn prepare_procfs_mock() -> anyhow::Result<TempDir> {
    let tmp = tempdir()?;

    let entries = [
        Entry {
            path: "1",
            entry_type: EntryType::Dir,
        },
        Entry {
            path: "2",
            entry_type: EntryType::Dir,
        },
        Entry {
            path: "3",
            entry_type: EntryType::Dir,
        },
        Entry {
            path: "1/cgroup",
            entry_type: EntryType::File("0::/system.slice/shared.slice"),
        },
        Entry {
            path: "2/cgroup",
            entry_type: EntryType::File("0::/system.slice/shared.slice"),
        },
        Entry {
            path: "3/cgroup",
            entry_type: EntryType::File("0::/system.slice/single.slice"),
        },
    ];

    let base_path = tmp.path().to_path_buf();

    create_mock_layout(base_path, &entries)?;
    Ok(tmp)
}

fn prepare_mock_measurements(ctx: &mut TransformCheckInputContext) -> anyhow::Result<MeasurementBuffer> {
    let metric_a = ctx
        .metrics()
        .by_name("metric_a")
        .expect("metric_a metric should exist")
        .0;
    let metric_b = ctx
        .metrics()
        .by_name("metric_b")
        .expect("metric_b metric should exist")
        .0;

    let mut m = MeasurementBuffer::new();

    let create_point = |timestamp, metric, consumer, value| {
        MeasurementPoint::new_untyped(
            timestamp,
            metric,
            Resource::LocalMachine,
            consumer,
            WrappedMeasurementValue::U64(value),
        )
    };

    // metric_a points
    m.push(create_point(*T, metric_a, ResourceConsumer::Process { pid: 1 }, 10));
    m.push(create_point(*T2, metric_a, ResourceConsumer::Process { pid: 1 }, 10));
    m.push(create_point(*T, metric_a, ResourceConsumer::Process { pid: 2 }, 10));
    m.push(create_point(*T, metric_a, ResourceConsumer::Process { pid: 3 }, 10));
    m.push(create_point(*T, metric_a, ResourceConsumer::LocalMachine, 10));

    // metric_b points
    m.push(create_point(*T, metric_b, ResourceConsumer::Process { pid: 1 }, 10));
    m.push(create_point(*T, metric_b, ResourceConsumer::Process { pid: 2 }, 10));

    Ok(m)
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Table::try_from(config).unwrap().clone()
}

// Mock filesystem utilities
enum EntryType {
    File(&'static str),
    Dir,
}

struct Entry {
    pub path: &'static str,
    pub entry_type: EntryType,
}

fn create_mock_layout(base_path: PathBuf, entries: &[Entry]) -> std::io::Result<()> {
    for entry in entries {
        let full_path = base_path.join(entry.path);
        match &entry.entry_type {
            EntryType::Dir => fs::create_dir_all(&full_path)?,
            EntryType::File(content) => {
                if let Some(parent) = full_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                let mut file = File::create(full_path)?;
                file.write_all(content.as_bytes())?;
            }
        }
    }
    Ok(())
}

// Mock plugin implementations
struct DumbNvmlPlugin;

impl AlumetPlugin for DumbNvmlPlugin {
    fn name() -> &'static str {
        "dumb"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let metric_a = alumet.create_metric("metric_a", Unit::Unity, "Some metric for tests purpose")?;
        let metric_b = alumet.create_metric("metric_b", Unit::Unity, "Some metric for tests purpose")?;

        let source = Box::new(DumbNvmlSource {
            _metric_a: metric_a,
            _metric_b: metric_b,
        });
        alumet.add_source("tests", source, TriggerSpec::at_interval(Duration::from_secs(1)))?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct DumbNvmlSource {
    _metric_a: TypedMetricId<u64>,
    _metric_b: TypedMetricId<u64>,
}

impl Source for DumbNvmlSource {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        Ok(())
    }
}
fn string_vec(values: &[&str]) -> Vec<String> {
    values.into_iter().map(|s| s.to_string()).collect()
}
