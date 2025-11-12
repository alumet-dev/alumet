use std::fs::{self};
use std::time::{Duration, UNIX_EPOCH};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::naming::OutputName,
    plugin::PluginMetadata,
    resources::{Resource, ResourceConsumer},
    test::{RuntimeExpectations, runtime::OutputCheckInputContext},
    units::Unit,
};
use indoc::indoc;

use plugin_csv::{Config, CsvPlugin};
use tempfile;

use pretty_assertions::assert_eq;

pub const TIMEOUT: Duration = Duration::from_secs(10);

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

struct TestMetrics {
    metric_u64: RawMetricId,
    metric_f64: RawMetricId,
}

impl TestMetrics {
    pub fn get(ctx: &OutputCheckInputContext) -> Self {
        Self {
            metric_u64: ctx.metrics().by_name("test_metric_u64").expect("metric should exist").0,
            metric_f64: ctx.metrics().by_name("test_metric_f64").expect("metric should exist").0,
        }
    }
}

fn simple_point(metric: RawMetricId, value: WrappedMeasurementValue) -> MeasurementPoint {
    MeasurementPoint::new_untyped(
        Timestamp::from(UNIX_EPOCH),
        metric,
        Resource::LocalMachine,
        ResourceConsumer::LocalMachine,
        value,
    )
}

#[test]
fn csv_output() {
    let _ = env_logger::Builder::from_default_env().try_init();

    // Prepare the plugin

    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let result_file = file_path.clone();
    let result_file_2 = result_file.clone();

    let config = Config {
        output_path: file_path.clone(),
        ..Config::default()
    };

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<CsvPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    // Prepare the scenarios

    let expected_string_no_late_attributes = indoc! {
        r#"metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;attributes_1;attributes_2;__late_attributes
           test_metric_u64;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;value1;value2;
           test_metric_u64;1970-01-01T00:00:00Z;1;local_machine;;local_machine;;value1;;
           test_metric_f64;1970-01-01T00:00:00Z;0.5;local_machine;;local_machine;;;value2;
           test_metric_f64;1970-01-01T00:00:00Z;0.75;local_machine;;local_machine;;;;
        "#
    };
    let expected_string_with_late_attributes = indoc! {
        r#"metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;attributes_1;attributes_2;__late_attributes
           test_metric_u64;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;value1;value2;
           test_metric_u64;1970-01-01T00:00:00Z;1;local_machine;;local_machine;;value1;;
           test_metric_f64;1970-01-01T00:00:00Z;0.5;local_machine;;local_machine;;;value2;
           test_metric_f64;1970-01-01T00:00:00Z;0.75;local_machine;;local_machine;;;;
           test_metric_u64;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;;;late_attributes_1=value1,late_attributes_2=value2
        "#
    };

    let elastic_output = OutputName::from_str("csv", "out");
    let runtime_expectations = RuntimeExpectations::new()
        .create_metric::<u64>("test_metric_u64", Unit::Unity)
        .create_metric::<f64>("test_metric_f64", Unit::Unity)
        .test_output(
            elastic_output.clone(),
            move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
                // Prepare input ( multiple points with/without attributes u64 or f64)
                let metrics = TestMetrics::get(ctx);

                let point1 = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0))
                    .with_attr("attributes_1", "value1")
                    .with_attr("attributes_2", "value2");
                let point2 = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(1))
                    .with_attr("attributes_1", "value1");
                let point3 = simple_point(metrics.metric_f64, WrappedMeasurementValue::F64(0.5))
                    .with_attr("attributes_2", "value2");
                let point4 = simple_point(metrics.metric_f64, WrappedMeasurementValue::F64(0.75));

                // Give input to the pipeline
                MeasurementBuffer::from(vec![point1, point2, point3, point4])
            },
            move || {
                assert_eq!(
                    fs::read_to_string(&result_file).unwrap(),
                    expected_string_no_late_attributes
                );
            },
        )
        // Adding another point to have late attributes
        .test_output(
            elastic_output.clone(),
            move |ctx| {
                let metrics = TestMetrics::get(ctx);
                let point1 = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0))
                    .with_attr("late_attributes_1", "value1")
                    .with_attr("late_attributes_2", "value2");

                // Give input to the pipeline
                MeasurementBuffer::from(vec![point1])
            },
            move || {
                assert_eq!(
                    fs::read_to_string(&result_file_2).unwrap(),
                    expected_string_with_late_attributes
                );
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn write_output_config_unit_unique_name() {
    let _ = env_logger::Builder::from_default_env().try_init();

    // Prepare the plugin

    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let result_file = file_path.clone();

    let config = Config {
        output_path: file_path.clone(),
        // not using the display name therefore using the unique name
        use_unit_display_name: false,
        ..Config::default()
    };

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<CsvPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    // Prepare the scenarios

    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ntest_metric_u64_1;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\n";

    let elastic_output = OutputName::from_str("csv", "out");

    let runtime_expectations = RuntimeExpectations::new()
        .create_metric::<u64>("test_metric_u64", Unit::Unity)
        .create_metric::<f64>("test_metric_f64", Unit::Unity)
        .test_output(
            elastic_output.clone(),
            {
                move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
                    // Prepare input ( multiple points with/without attributes u64 or f64)
                    let metrics = TestMetrics::get(ctx);
                    let point1 = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0));
                    // Give input to the pipeline
                    MeasurementBuffer::from(vec![point1])
                }
            },
            {
                move || {
                    assert_eq!(fs::read_to_string(&result_file).unwrap(), expected_string);
                }
            },
        );
    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
#[test]
fn write_output_config_no_unit_in_metric_name() {
    let _ = env_logger::Builder::from_default_env().try_init();

    // Prepare the plugin

    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let result_file = file_path.clone();

    let config = Config {
        output_path: file_path.clone(),
        append_unit_to_metric_name: false,
        ..Config::default()
    };

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<CsvPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    // Prepare the scenarios

    let expected_string = indoc! {
    r#"metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes
           test_metric_u64;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;
        "#};

    let elastic_output = OutputName::from_str("csv", "out");

    let runtime_expectations = RuntimeExpectations::new()
        .create_metric::<u64>("test_metric_u64", Unit::Second)
        .create_metric::<f64>("test_metric_f64", Unit::Unity)
        .test_output(
            elastic_output.clone(),
            move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
                // Prepare input
                let metrics = TestMetrics::get(ctx);
                let point1 = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0));
                // Give input to the pipeline
                MeasurementBuffer::from(vec![point1])
            },
            move || {
                assert_eq!(fs::read_to_string(&result_file).unwrap(), expected_string);
            },
        );
    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
