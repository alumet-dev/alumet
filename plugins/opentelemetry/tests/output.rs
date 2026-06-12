pub mod fakeplugin;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, UNIX_EPOCH},
};

use tokio::net::TcpListener;
use tonic::{Request, Response, Status, transport::Server};

use opentelemetry_proto::tonic::{
    collector::metrics::v1::{
        ExportMetricsServiceRequest, ExportMetricsServiceResponse,
        metrics_service_server::{MetricsService, MetricsServiceServer},
    },
    common::v1::any_value,
    metrics::v1::Metric,
};

use plugin_opentelemetry::{Config, OpenTelemetryPlugin};

use crate::fakeplugin::TestsPlugin;
use serial_test::serial;

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    pipeline::naming::OutputName,
    plugin::PluginMetadata,
    resources::{Resource, ResourceConsumer},
    test::{RuntimeExpectations, runtime::OutputCheckInputContext},
};

// ---------------------------------------------------------------------------
// Mock gRPC server
// ---------------------------------------------------------------------------

/// Captures every ExportMetricsServiceRequest received by the mock collector.
#[derive(Default, Clone)]
struct MockCollector {
    received: Arc<Mutex<Vec<ExportMetricsServiceRequest>>>,
}

impl MockCollector {
    fn new() -> Self {
        Self {
            received: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns a snapshot of all requests received so far.
    fn requests(&self) -> Vec<ExportMetricsServiceRequest> {
        self.received.lock().unwrap().clone()
    }
}

#[tonic::async_trait]
impl MetricsService for MockCollector {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        self.received.lock().unwrap().push(request.into_inner());
        Ok(Response::new(ExportMetricsServiceResponse { partial_success: None }))
    }
}

/// Spawns a mock OTLP gRPC collector on a random free port.
/// Returns the collector handle (to inspect received data) and the bound address.
async fn spawn_mock_collector() -> (MockCollector, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let collector = MockCollector::new();
    let collector_clone = collector.clone();

    tokio::spawn(async move {
        Server::builder()
            .add_service(MetricsServiceServer::new(collector_clone))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    (collector, addr)
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Retrieves a string attribute value from a slice of `KeyValue` pairs.
/// Returns `Some(&str)` if the key is found and its value is a `StringValue`,
/// or `None` otherwise.
fn attr_str<'a>(attrs: &'a [opentelemetry_proto::tonic::common::v1::KeyValue], key: &str) -> Option<&'a str> {
    attrs.iter().find(|kv| kv.key == key).and_then(|kv| {
        kv.value.as_ref().and_then(|v| match &v.value {
            Some(any_value::Value::StringValue(s)) => Some(s.as_str()),
            _ => None,
        })
    })
}

/// Serialises a [`Config`] into a `toml::Table`, as expected by [`PluginInfo`].
fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

/// Builds a [`PluginSet`] containing both the `OpenTelemetryPlugin` and the `TestsPlugin`.
fn add_plugins(plugin_config: Config) -> PluginSet {
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<OpenTelemetryPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&plugin_config)),
    });
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<TestsPlugin>(),
        enabled: true,
        config: None,
    });
    plugins
}

/// Searches all received requests for a metric with the given `name`, flattening
/// across `resource_metrics` → `scope_metrics` → `metrics`.
/// Panics with a descriptive message if the metric is not found.
fn find_metric(collector: &MockCollector, name: &str) -> Metric {
    let requests = collector.requests();
    requests
        .iter()
        .flat_map(|req| &req.resource_metrics)
        .flat_map(|rm| &rm.scope_metrics)
        .flat_map(|sm| &sm.metrics)
        .find(|m| m.name == name)
        .cloned()
        .unwrap_or_else(|| panic!("metric '{name}' missing"))
}

/// Runs an Alumet agent and checks the `"opentelemetry/out"` output.
/// `make_input` produces the measurements to feed in, `check_output` asserts on the result.
/// Blocks until shutdown (timeout: 5 s).
fn run_agent(
    plugins: PluginSet,
    make_input: impl Fn(&mut OutputCheckInputContext) -> MeasurementBuffer + Send + 'static,
    check_output: impl Fn() + Send + 'static,
) {
    let runtime_expectations =
        RuntimeExpectations::new().test_output(OutputName::from_str("opentelemetry", "out"), make_input, check_output);

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Basic smoke-test: a single U64 measurement is exported with the correct
/// metric name, value, timestamp, and resource attributes.
#[test]
#[serial]
fn write_single_u64_measurement() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let measure_timestamp_ns: u64 = 818254800_000_000_000;
    let measure_timestamp = UNIX_EPOCH + Duration::from_nanos(measure_timestamp_ns);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(measure_timestamp),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(42),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let requests = collector_for_check.requests();
        assert!(
            !requests.is_empty(),
            "collector should have received at least one request"
        );

        let metric = find_metric(&collector_for_check, "dummy");

        // Check that it is exported as a Gauge
        let gauge = match &metric.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g)) => g,
            other => panic!("expected Gauge, got {:?}", other),
        };

        assert_eq!(gauge.data_points.len(), 1);
        let dp = &gauge.data_points[0];

        // Timestamp
        assert_eq!(dp.time_unix_nano, measure_timestamp_ns, "timestamp mismatch");

        // Value (U64 is cast to f64 in the output)
        match &dp.value {
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v)) => {
                assert!((*v - 42.0_f64).abs() < f64::EPSILON, "value mismatch: {v}");
            }
            other => panic!("unexpected value type: {:?}", other),
        }

        // Resource attributes
        let attrs = &dp.attributes;
        assert_eq!(
            attr_str(attrs, "resource_kind"),
            Some("local_machine"),
            "resource_kind mismatch"
        );
        assert_eq!(
            attr_str(attrs, "resource_consumer_kind"),
            Some("local_machine"),
            "resource_consumer_kind mismatch"
        );
    };

    run_agent(plugins, make_input, check_output);
}

/// F64 measurements are forwarded without loss (within floating-point precision).
#[test]
#[serial]
fn write_single_f64_measurement() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(3.14),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let metric = find_metric(&collector_for_check, "dummy");

        // Check that it is exported as a Gauge
        let gauge = match &metric.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g)) => g,
            other => panic!("expected Gauge, got {:?}", other),
        };

        // Value
        let dp = &gauge.data_points[0];
        match dp.value {
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v)) => {
                assert!((v - 3.14_f64).abs() < 1e-9, "f64 value mismatch: {v}");
            }
            other => panic!("unexpected value type: {:?}", other),
        }
    };

    run_agent(plugins, make_input, check_output);
}

/// Tests that multiple measurements for the same metric name are grouped into
/// a single OTLP Metric object with multiple data points, while different
/// metric names remain in separate objects.
#[test]
#[serial]
fn group_multiple_datapoints_under_same_metric() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: true,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        // We use two different metrics: "dummy" and "other"
        let metric_a = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let metric_b = ctx.metrics().by_name("other").expect("metric should exist").0;

        let mut buf = MeasurementBuffer::new();
        let ts = Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000));

        // 1. First data point for "dummy"
        let mut p1 = MeasurementPoint::new_untyped(
            ts,
            metric_a,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(10),
        );
        p1.add_attr("id", "point_1");
        buf.push(p1);

        // 2. Second data point for "dummy"
        let mut p2 = MeasurementPoint::new_untyped(
            ts,
            metric_a,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(20),
        );
        p2.add_attr("id", "point_2");
        buf.push(p2);

        // 3. One data point for "other"
        let p3 = MeasurementPoint::new_untyped(
            ts,
            metric_b,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(30),
        );
        buf.push(p3);

        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let requests = collector_for_check.requests();
        let all_metrics: Vec<_> = requests
            .iter()
            .flat_map(|req| &req.resource_metrics)
            .flat_map(|rm| &rm.scope_metrics)
            .flat_map(|sm| &sm.metrics)
            .collect();

        // Check overall count: should have exactly 2 Metric objects (one for "dummy", one for "other")
        assert_eq!(
            all_metrics.len(),
            2,
            "Should have grouped measurements into exactly 2 Metric objects"
        );

        // --- Verify "dummy" grouping ---
        let dummy_metric = all_metrics
            .iter()
            .find(|m| m.name == "dummy")
            .expect("Metric 'dummy' not found");
        let dummy_gauge = match &dummy_metric.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g)) => g,
            _ => panic!("Expected Gauge data"),
        };
        assert_eq!(
            dummy_gauge.data_points.len(),
            2,
            "Metric 'dummy' should have 2 data points"
        );

        // Verify values are both present
        let values: Vec<f64> = dummy_gauge
            .data_points
            .iter()
            .map(|dp| match dp.value {
                Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v)) => v,
                _ => 0.0,
            })
            .collect();
        assert!(values.contains(&10.0));
        assert!(values.contains(&20.0));

        // --- Verify "other" grouping ---
        let other_metric = all_metrics
            .iter()
            .find(|m| m.name == "other")
            .expect("Metric 'other' not found");
        let other_gauge = match &other_metric.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g)) => g,
            _ => panic!("Expected Gauge data"),
        };
        assert_eq!(
            other_gauge.data_points.len(),
            1,
            "Metric 'other' should have 1 data point"
        );

        // Verify the values is present
        let values: Vec<f64> = other_gauge
            .data_points
            .iter()
            .map(|dp| match dp.value {
                Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v)) => v,
                _ => 0.0,
            })
            .collect();
        assert!(values.contains(&30.0));
    };

    run_agent(plugins, make_input, check_output);
}

/// Prefix and suffix are correctly prepended / appended to the metric name.
#[test]
#[serial]
fn metric_name_prefix_and_suffix() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: "pre_".to_string(),
        suffix: "_suf".to_string(),
        use_unit_display_name: false,
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        find_metric(&collector_for_check, "pre_dummy_suf");
    };

    run_agent(plugins, make_input, check_output);
}

/// When `use_unit_display_name` is false, the unit unique_name is used.
#[test]
#[serial]
fn use_unit_display_name_false() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false, // <-- disabled
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let metric = find_metric(&collector_for_check, "dummy");

        assert_eq!(
            metric.unit, "g_CO2",
            "expected display name 'g_CO2', got '{}'",
            metric.unit
        );
    };

    run_agent(plugins, make_input, check_output);
}

/// When `use_unit_display_name` is true, the unit display_name is used instead of
/// the unique_name for units.
#[test]
#[serial]
fn use_unit_display_name_true() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: true, // <-- enabled
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let metric = find_metric(&collector_for_check, "dummy");

        assert_eq!(
            metric.unit, "gCO₂",
            "expected display name 'gCO₂', got '{}'",
            metric.unit
        );
    };

    run_agent(plugins, make_input, check_output);
}

/// When `add_attributes_to_labels` is true, measurement attributes are
/// forwarded as data-point attributes.
#[test]
#[serial]
fn measurement_attributes_forwarded_when_enabled() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: true, // <-- enabled
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        let mut point = MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(7),
        );
        point.add_attr("host", "node-01");
        point.add_attr("env", "prod");
        buf.push(point);
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let dp = find_metric(&collector_for_check, "dummy")
            .data
            .and_then(|d| match d {
                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g) => g.data_points.into_iter().next(),
                _ => None,
            })
            .expect("data point not found");

        assert_eq!(
            attr_str(&dp.attributes, "host"),
            Some("node-01"),
            "missing 'host' attribute"
        );
        assert_eq!(attr_str(&dp.attributes, "env"), Some("prod"), "missing 'env' attribute");
    };

    run_agent(plugins, make_input, check_output);
}

/// Empty label values must be replaced with the string `"empty"` to comply
/// with the OpenTelemetry specification.
#[test]
#[serial]
fn empty_attribute_values_replaced_with_empty_string_literal() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: true,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        let mut point = MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            // ResourceConsumer::LocalMachine produces an empty id_string
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        );
        // Explicitly add an attribute with an empty value
        point.add_attr("might_be_empty", "");
        buf.push(point);
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let dp = find_metric(&collector_for_check, "dummy")
            .data
            .and_then(|d| match d {
                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g) => g.data_points.into_iter().next(),
                _ => None,
            })
            .expect("data point not found");

        // Every string attribute must be non-empty
        for kv in &dp.attributes {
            if let Some(v) = &kv.value {
                if let Some(any_value::Value::StringValue(s)) = &v.value {
                    assert!(
                        !s.is_empty(),
                        "attribute '{}' has an empty string value, should be 'empty'",
                        kv.key
                    );
                }
            }
        }

        // The explicitly-empty attribute must have been replaced
        assert_eq!(
            attr_str(&dp.attributes, "might_be_empty"),
            Some("empty"),
            "'might_be_empty' attribute should have been replaced with 'empty'"
        );
    };

    run_agent(plugins, make_input, check_output);
}

/// Exporting an empty MeasurementBuffer must not send any gRPC request.
#[test]
#[serial]
fn empty_measurement_buffer_sends_no_request() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    // Return an empty buffer
    let make_input = |_ctx: &mut OutputCheckInputContext| -> MeasurementBuffer { MeasurementBuffer::new() };

    let collector_for_check = collector.clone();
    let check_output = move || {
        assert!(
            collector_for_check.requests().is_empty(),
            "no gRPC request should have been sent for an empty buffer"
        );
    };

    run_agent(plugins, make_input, check_output);
}

/// The InstrumentationScope embedded in every request must identify alumet,
/// and the Resource must declare the correct service name.
#[test]
#[serial]
fn instrumentation_scope_identifies_alumet() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (collector, addr) = rt.block_on(spawn_mock_collector());

    let plugin_config = Config {
        collector_host: format!("http://{}", addr),
        prefix: String::new(),
        suffix: String::new(),
        use_unit_display_name: false,
        add_attributes_to_labels: false,
    };
    let plugins = add_plugins(plugin_config);

    let make_input = move |ctx: &mut OutputCheckInputContext| -> MeasurementBuffer {
        let metric = ctx.metrics().by_name("dummy").expect("metric should exist").0;
        let mut buf = MeasurementBuffer::new();
        buf.push(MeasurementPoint::new_untyped(
            Timestamp::from(UNIX_EPOCH + Duration::from_nanos(1_000_000_000)),
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(1),
        ));
        buf
    };

    let collector_for_check = collector.clone();
    let check_output = move || {
        let requests = collector_for_check.requests();
        assert!(
            !requests.is_empty(),
            "collector should have received at least one request"
        );

        // --- Resource: service.name must be "alumet-otlp-grpc" ---
        let resource_metrics: Vec<_> = requests.iter().flat_map(|req| &req.resource_metrics).collect();

        assert!(!resource_metrics.is_empty(), "no ResourceMetrics found");

        for rm in &resource_metrics {
            let resource = rm.resource.as_ref().expect("ResourceMetrics must have a Resource");
            assert_eq!(
                attr_str(&resource.attributes, "service.name"),
                Some("alumet-otlp-grpc"),
                "resource attribute 'service.name' should be 'alumet-otlp-grpc'"
            );
        }

        // --- InstrumentationScope: name, version and tool attribute ---
        let scopes: Vec<_> = requests
            .iter()
            .flat_map(|req| &req.resource_metrics)
            .flat_map(|rm| &rm.scope_metrics)
            .filter_map(|sm| sm.scope.as_ref())
            .cloned()
            .collect();

        assert!(!scopes.is_empty(), "no InstrumentationScope found");

        for scope in &scopes {
            assert_eq!(scope.name, "alumet", "scope name should be 'alumet'");
            assert!(!scope.version.is_empty(), "scope version must not be empty");
            assert_eq!(
                attr_str(&scope.attributes, "tool"),
                Some("alumet"),
                "scope attribute 'tool' should be 'alumet'"
            );
        }
    };

    run_agent(plugins, make_input, check_output);
}
