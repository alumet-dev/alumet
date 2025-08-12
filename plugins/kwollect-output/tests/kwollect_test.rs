pub mod fakeplugin;

use alumet::agent::plugin::PluginInfo;
use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    pipeline::naming::OutputName,
    plugin::{rust::AlumetPlugin, PluginMetadata},
    resources::{Resource, ResourceConsumer},
    test::{runtime::OutputCheckInputContext, RuntimeExpectations},
};
use base64::prelude::*;
use mockito::{Mock, Server, ServerGuard};
use plugin_kwollect_output::{Config, KwollectPlugin};
use std::time::Duration;

use crate::fakeplugin::TestsPlugin;

fn mock_api_write(server: &mut ServerGuard, login: Option<&str>, password: Option<&str>) -> Mock {
    let mut server = server.mock("POST", "/");

    if let (Some(user), Some(pass)) = (login, password) {
        let base64_encoded = BASE64_STANDARD.encode(format!("{}:{}", user, pass));
        let auth = format!("Basic {}", base64_encoded);
        server = server.with_header("Authorization", &auth);
    }
    server
        .with_header(
            "User-Agent",
            "curl/7.21.2 (x86_64-apple-darwin10.4.0) libcurl/7.21.2 OpenSSL/1.0.0a zlib/1.2.5 libidn/1.19",
        )
        .with_header("Host", "api.soap.fr")
        .with_header("Accept", "*/*")
        .with_status(200)
        .with_body(
            r#"[{"metric_id": "my_custom_metric_1", "value": 42}, {"metric_id": "my_custom_metric_2", "value": 43}]"#,
        )
        .create()
}

#[test]
fn test_with_no_auth() {
    let mut server = Server::new();

    let mut plugins = PluginSet::new();
    let mut config = Config::default();
    config.url = server.url();
    config.login = None;
    config.password = None;
    config.hostname = Some("DHARMA".to_string());

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<KwollectPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<TestsPlugin>(),
        enabled: true,
        config: None,
    });
    let ts = Timestamp::now();
    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("example_counter").expect("metric should exist").0;
        let mut m = MeasurementBuffer::new();
        let test_point = MeasurementPoint::new_untyped(
            ts,
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(10),
        )
        .with_attr("agence", AttributeValue::String("CHERUB".to_string()))
        .with_attr("Fondation", AttributeValue::U64(1946));
        m.push(test_point);
        m
    };

    let test_write_mock = mock_api_write(&mut server, None, None);
    let check_output = move || {
        test_write_mock.assert();
    };

    let runtime_expectations = RuntimeExpectations::new().test_output(
        OutputName::from_str("kwollect-output", "kwollect-output"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn test_with_auth() {
    let mut server = Server::new();

    let mut plugins = PluginSet::new();
    let config = Config {
        url: server.url(),
        login: Some("toto".to_string()),
        password: Some("tata".to_string()),
        hostname: Some("DHARMA".to_string()),
        append_unit_to_metric_name: true,
        use_unit_display_name: false,
    };

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<KwollectPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<TestsPlugin>(),
        enabled: true,
        config: None,
    });
    let ts = Timestamp::now();
    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("example_counter").expect("metric should exist").0;
        let mut m = MeasurementBuffer::new();
        let test_point = MeasurementPoint::new_untyped(
            ts,
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(10),
        );
        m.push(test_point);
        m
    };

    let test_write_mock = mock_api_write(&mut server, None, None);
    let check_output = move || {
        test_write_mock.assert();
    };

    let runtime_expectations = RuntimeExpectations::new().test_output(
        OutputName::from_str("kwollect-output", "kwollect-output"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();
}

#[test]
fn test_default_config() {
    let _ = crate::KwollectPlugin::init(KwollectPlugin::default_config().unwrap().unwrap()).unwrap();
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}
