use alumet::agent::plugin::{PluginInfo, PluginSet};
use alumet::agent::{self};
use alumet::measurement::MeasurementBuffer;
use alumet::measurement::MeasurementPoint;
use alumet::measurement::{Timestamp, WrappedMeasurementValue};
use alumet::plugin::PluginMetadata;
use alumet::resources::{Resource, ResourceConsumer};
use alumet::test::runtime::OutputCheckInputContext;
use std::io::Read;
use std::time::{Duration, UNIX_EPOCH};
use std::{fs::File, io::BufReader};

use plugin_csv::{Config, CsvPlugin};

use alumet::pipeline::naming::OutputName;
use alumet::test::RuntimeExpectations;

use super::fakeplugin::TestsPlugin;

pub const TIMEOUT: Duration = Duration::from_secs(15);

pub fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

fn compare_file_and_string(file_path_1: &str, expected_string: &str) -> Result<(), anyhow::Error> {
    let file_1 = File::open(file_path_1)?;
    let mut buf_reader_1 = BufReader::new(file_1);
    let mut contents_1 = String::new();
    buf_reader_1.read_to_string(&mut contents_1)?;

    assert_eq!(contents_1, expected_string);
    Ok(())
}

pub fn helper_check_output(result_file: String, expected_string: String) -> impl Fn() + Send + 'static {
    return move || {
        let res = compare_file_and_string(result_file.as_str(), expected_string.as_str());
        match res {
            Ok(()) => (),
            Err(e) => panic!("Error: {e:?}"),
        };
    };
}
fn helper_add_plugins(config: Config, plugins: &mut PluginSet) {
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<CsvPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<TestsPlugin>(),
        enabled: true,
        config: None,
    });
}
pub fn helper_make_input(
    value: WrappedMeasurementValue,
    point_with_attributes: bool,
) -> impl Fn(&mut OutputCheckInputContext) -> MeasurementBuffer {
    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dumb").expect("metric should exist").0;
        let mut m = MeasurementBuffer::new();
        let mut test_point: MeasurementPoint;

        test_point = MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            value.clone(),
        );

        if point_with_attributes {
            test_point.add_attr("attributes_1", "value1");
            test_point.add_attr("attributes_2", "value2");
        }
        m.push(test_point);
        m
    };
    make_input
}
pub fn helper_test_write_output_config(
    config: Config,
    make_input: impl Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
    check_output: impl Fn() + Send + 'static,
) {
    let mut plugins = PluginSet::new();
    helper_add_plugins(config, &mut plugins);

    let runtime_expectations =
        RuntimeExpectations::new().test_output(OutputName::from_str("csv", "out"), make_input, check_output);

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
pub fn helper_test_write_output_config_for_late_attributes(
    config: Config,
    make_input: impl Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
    check_output: impl Fn() + Send + 'static,
    make_input_2: impl Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
    check_output_2: impl Fn() + Send + 'static,
) {
    let mut plugins = PluginSet::new();
    helper_add_plugins(config, &mut plugins);

    let runtime_expectations = RuntimeExpectations::new()
        .test_output(OutputName::from_str("csv", "out"), make_input, check_output)
        .test_output(OutputName::from_str("csv", "out"), make_input_2, check_output_2);

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
