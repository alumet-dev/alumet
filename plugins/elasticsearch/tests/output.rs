use std::{
    sync::{Arc, Mutex},
    time::{Duration, UNIX_EPOCH},
};

use alumet::agent::{
    self,
    plugin::{PluginInfo, PluginSet},
};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::naming::OutputName,
    plugin::PluginMetadata,
    resources::{Resource, ResourceConsumer},
    test::{RuntimeExpectations, runtime::OutputCheckInputContext},
    units::Unit,
};

use mockito::Mock;
use plugin_elasticsearch::{ElasticSearchPlugin, plugin::config::Config};

use indoc::indoc;

const TIMEOUT: Duration = Duration::from_secs(15);

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

#[derive(Debug)]
struct SharedMocks {
    server: mockito::Server,
    create_index_template: Mock,
    current_mock: Option<Mock>,
}

impl SharedMocks {
    fn mock_elastic_put(&mut self, expected_body: &str) {
        self.current_mock = Some(
            self.server
                .mock("PUT", "/_bulk")
                .match_header("content-type", "application/x-ndjson")
                .match_body(expected_body)
                .with_status(200)
                .create(),
        );
    }
}

#[test]
fn elastic_output() {
    let _ = env_logger::Builder::from_default_env().try_init();

    // Mocking elasticsearch server
    let mut server = mockito::Server::new_with_opts(mockito::ServerOpts {
        host: "127.0.0.1",
        ..Default::default()
    });

    // Mock api request to create_index_template (expected once on startup)
    let create_index_template: Mock = server
        .mock("PUT", "/_index_template/alumet_index_template")
        .with_status(200)
        .create();

    // Debug mock
    let _ = server
        .mock("PUT", "/_bulk")
        .match_request(|request| {
            log::debug!("the body of the received request is : {:?}", request.utf8_lossy_body());
            false
        })
        .with_status(200)
        .create()
        .expect(0);

    // Prepare the plugin
    let config = Config {
        server_url: server.url(),
        ..Config::default()
    };

    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<ElasticSearchPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    // Share the mocking structures between each scenario
    let mocks = Arc::new(Mutex::new(SharedMocks {
        server,
        create_index_template,
        current_mock: None, // will be setup in each RuntimeExpectations scenario
    }));

    // Prepare the scenarios
    let elastic_output = OutputName::from_str("elasticsearch", "api");
    let runtime_expectations = RuntimeExpectations::new()
        .create_metric::<u64>("test_metric_u64", Unit::Unity)
        .create_metric::<f64>("test_metric_f64", Unit::Unity)
        .test_output(
            elastic_output.clone(),
            {
                let mocks = mocks.clone();
                move |ctx| {

                    // Prepare input (point u64 without attributes)
                    let metrics = TestMetrics::get(ctx);
                    let point = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0));

                    // Prepare mock
                    let expected_body = indoc!{
                        r#"{"create":{"_index":"alumet-test_metric_u64"}}
                        {"@timestamp":"1970-01-01T00:00:00Z","resource_kind":"local_machine","resource_id":"","consumer_kind":"local_machine","consumer_id":"","value":0}
                        "#};
                    mocks.lock().unwrap().mock_elastic_put(expected_body);

                    // Give input to the pipeline
                    MeasurementBuffer::from(vec![point])
                }
            },
            {
                let mocks = mocks.clone();
                move || {
                    let mocks = mocks.lock().unwrap();
                    mocks.create_index_template.assert();
                    mocks.current_mock.as_ref().unwrap().assert();
                }
            },
        )
        .test_output(
            elastic_output.clone(),
            {
                let mocks = mocks.clone();
                move |ctx|  {

                    // Prepare input (point u64 with attributes)
                    let metrics = TestMetrics::get(ctx);
                    let point = simple_point(metrics.metric_u64, WrappedMeasurementValue::U64(0))
                        .with_attr("attributes_1", "value1");

                    // Prepare mock
                    let expected_body = indoc! {
                        r#"{"create":{"_index":"alumet-test_metric_u64"}}
                        {"@timestamp":"1970-01-01T00:00:00Z","resource_kind":"local_machine","resource_id":"","consumer_kind":"local_machine","consumer_id":"","value":0,"attributes_1":"value1"}
                        "#};
                    mocks.lock().unwrap().mock_elastic_put(expected_body);

                    // Give input to the pipeline
                    MeasurementBuffer::from(vec![point])
                }
            },
            {
                let mocks = mocks.clone();
                move || {
                    let mocks = mocks.lock().unwrap();
                    mocks.create_index_template.assert();
                    mocks.current_mock.as_ref().unwrap().assert();
                }
            })
        .test_output(
            elastic_output.clone(),
            {
                let mocks = mocks.clone();
                move |ctx| {

                    // Prepare input (point f64 without attributes )
                    let metrics = TestMetrics::get(ctx);
                    let point = simple_point(metrics.metric_f64, WrappedMeasurementValue::F64(0.5));

                    // Prepare mock
                    let expected_body = indoc!{
                        r#"{"create":{"_index":"alumet-test_metric_f64"}}
                        {"@timestamp":"1970-01-01T00:00:00Z","resource_kind":"local_machine","resource_id":"","consumer_kind":"local_machine","consumer_id":"","value":0.5}
                        "#};
                    mocks.lock().unwrap().mock_elastic_put(expected_body);

                    // Give input to the pipeline
                    MeasurementBuffer::from(vec![point])
                }
            },
            {
                let mocks = mocks.clone();
                move || {
                    let mocks = mocks.lock().unwrap();
                    mocks.create_index_template.assert();
                    mocks.current_mock.as_ref().unwrap().assert();
                }
            },
        );
    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
