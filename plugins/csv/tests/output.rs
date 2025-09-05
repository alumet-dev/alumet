pub mod common;

use alumet::measurement::WrappedMeasurementValue;
use common::helper::*;
use plugin_csv::Config;
use tempfile;

#[test]
fn test_write_point_without_attributes() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");
    let config = Config {
        output_path: file_path.clone(),
        ..Config::default()
    };

    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\n";

    let value_test_point = WrappedMeasurementValue::U64(0);
    let point_with_attributes = false;

    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.clone().into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config(config, make_input, check_output);
}

#[test]
fn test_write_point_some_attributes() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");
    let config = Config {
        output_path: file_path.clone(),
        ..Config::default()
    };

    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;attributes_1;attributes_2;__late_attributes\ndumb;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;value1;value2;\n";

    let value_test_point = WrappedMeasurementValue::U64(0);
    let point_with_attributes = true;

    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config(config, make_input, check_output);
}

#[test]
fn test_write_output_config_unit_unique_name() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let config = Config {
        output_path: file_path.clone(),
        // not using the display name therefore using the unique name
        use_unit_display_name: false,
        ..Config::default()
    };
    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb_1;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\n";

    let value_test_point = WrappedMeasurementValue::U64(0);
    let point_with_attributes = false;
    //let make_input = helper_make_input(&value_test_point, &point_with_attributes);
    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config(config, make_input, check_output);
}

#[test]
fn test_write_output_config_no_unit_in_metric_name() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let config = Config {
        output_path: file_path.clone(),
        append_unit_to_metric_name: false,
        ..Config::default()
    };
    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\n";

    let value_test_point = WrappedMeasurementValue::U64(0);
    let point_with_attributes = false;

    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config(config, make_input, check_output);
}

#[test]
fn test_write_output_measurement_f64() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let config = Config {
        output_path: file_path.clone(),
        ..Config::default()
    };
    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb;1970-01-01T00:00:00Z;0.5;local_machine;;local_machine;;\n";

    let value_test_point = WrappedMeasurementValue::F64(0.5);
    let point_with_attributes = false;

    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config(config, make_input, check_output);
}

#[test]
fn test_write_output_late_attributes() {
    // First step : adding a point without attributes

    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("alumet-output.csv");

    let config = Config {
        output_path: file_path.clone(),
        ..Config::default()
    };
    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\n";
    let value_test_point = WrappedMeasurementValue::U64(0);
    let point_with_attributes = false;

    let make_input = helper_make_input(value_test_point, point_with_attributes);
    let check_output = helper_check_output(
        file_path.clone().into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    //  Second step : adding a point with attributes

    let expected_string = "metric;timestamp;value;resource_kind;resource_id;consumer_kind;consumer_id;__late_attributes\ndumb;1970-01-01T00:00:00Z;0;local_machine;;local_machine;;\ndumb;1970-01-01T00:00:00Z;0.5;local_machine;;local_machine;;attributes_1=value1, attributes_2=value2\n";

    let value_test_point = WrappedMeasurementValue::F64(0.5);
    let point_with_attributes = true;

    let make_input_2 = helper_make_input(value_test_point, point_with_attributes);
    let check_output_2 = helper_check_output(
        file_path.into_os_string().into_string().unwrap(),
        expected_string.to_string(),
    );

    helper_test_write_output_config_for_late_attributes(config, make_input, check_output, make_input_2, check_output_2);
}
