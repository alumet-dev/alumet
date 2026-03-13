use std::time::Duration;

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::{RawMetricId, registry::MetricRegistry},
    pipeline::naming::TransformName,
    plugin::PluginMetadata,
    resources::{Resource, ResourceConsumer},
    test::RuntimeExpectations,
    units::Unit,
};

use plugin_filter::FilterPlugin;

const TIMEOUT: Duration = Duration::from_secs(2);

const CONFIG_INCLUDE: &str = r#"
include = ["metric_a"]
"#;

const CONFIG_EXCLUDE: &str = r#"
exclude = ["metric_a"]
"#;

#[test]
fn test_filter_include() {
    let transform_name = TransformName::from_str("filter", "transform");
    let ts1 = Timestamp::now();
    let ts2 = ts1 + Duration::from_secs(1);

    let runtime = RuntimeExpectations::new()
        .create_metric::<u64>("metric_a", Unit::Unity)
        .create_metric::<u64>("metric_b", Unit::Unity)
        .test_transform(
            transform_name.clone(),
            move |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();

                buf.push(new_point(&metrics, metrics.metric_a, ts1, 1));
                buf.push(new_point(&metrics, metrics.metric_a, ts2, 1));
                buf.push(new_point(&metrics, metrics.metric_b, ts1, 2));
                buf.push(new_point(&metrics, metrics.metric_b, ts2, 2));

                buf
            },
            move |output| {
                let metrics = TestMetrics::find_in(output.metrics());
                let m = output.measurements().to_vec();

                assert_eq!(
                    m,
                    vec![
                        new_point(&metrics, metrics.metric_a, ts1, 1),
                        new_point(&metrics, metrics.metric_a, ts2, 1),
                    ]
                );
            },
        );

    run_agent(runtime, CONFIG_INCLUDE);
}

#[test]
fn test_filter_exclude() {
    let transform_name = TransformName::from_str("filter", "transform");
    let ts1 = Timestamp::now();
    let ts2 = ts1 + Duration::from_secs(1);

    let runtime = RuntimeExpectations::new()
        .create_metric::<u64>("metric_a", Unit::Unity)
        .create_metric::<u64>("metric_b", Unit::Unity)
        .test_transform(
            transform_name.clone(),
            move |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();

                buf.push(new_point(&metrics, metrics.metric_a, ts1, 1));
                buf.push(new_point(&metrics, metrics.metric_a, ts2, 1));
                buf.push(new_point(&metrics, metrics.metric_b, ts1, 2));
                buf.push(new_point(&metrics, metrics.metric_b, ts2, 2));

                buf
            },
            move |output| {
                let metrics = TestMetrics::find_in(output.metrics());
                let m = output.measurements().to_vec();

                assert_eq!(
                    m,
                    vec![
                        new_point(&metrics, metrics.metric_b, ts1, 2),
                        new_point(&metrics, metrics.metric_b, ts2, 2),
                    ]
                );
            },
        );

    run_agent(runtime, CONFIG_EXCLUDE);
}

fn run_agent(runtime: RuntimeExpectations, config: &str) {
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<FilterPlugin>(),
        enabled: true,
        config: if config.is_empty() {
            None
        } else {
            Some(toml::from_str(config).unwrap())
        },
    });

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

fn new_point(metrics: &TestMetrics, metric: RawMetricId, ts: Timestamp, value: u64) -> MeasurementPoint {
    MeasurementPoint::new_untyped(
        ts,
        metric,
        Resource::LocalMachine,
        ResourceConsumer::LocalMachine,
        WrappedMeasurementValue::U64(value),
    )
}

struct TestMetrics {
    metric_a: RawMetricId,
    metric_b: RawMetricId,
}

impl TestMetrics {
    fn find_in(metrics: &MetricRegistry) -> Self {
        Self {
            metric_a: metrics.by_name("metric_a").unwrap().0,
            metric_b: metrics.by_name("metric_b").unwrap().0,
        }
    }
}
