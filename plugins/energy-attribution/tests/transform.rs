//! Integration tests for the energy attribution transform.

use std::time::{Duration, SystemTime};

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
use plugin_energy_attribution::EnergyAttributionPlugin;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const TIMEOUT: Duration = Duration::from_secs(2);
const CONFIG_CPU: &str = r#"
        [formulas.attributed_energy]
        expr = "cpu_energy * cpu_usage / 100.0"
        ref = "cpu_energy"

        [formulas.attributed_energy.per_resource]
        cpu_energy = { metric = "rapl_consumed_energy", resource_kind = "local_machine", domain = "package_total" }

        [formulas.attributed_energy.per_consumer]
        cpu_usage = { metric = "cpu_usage_percent", kind = "total" }
    "#;

#[test]
fn test_cpu_energy_to_processes() {
    init_logger();
    let attribution_transform = TransformName::from_str("energy-attribution", "attribution/attributed_energy");

    fn new_point_energy(metrics: &TestMetrics, timestamp: &str, value: u64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            metrics.rapl_consumed_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::U64(value),
        )
        .with_attr("domain", "package_total")
    }

    fn new_point_usage(metrics: &TestMetrics, timestamp: &str, pid: u32, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            metrics.cpu_usage_percent,
            Resource::LocalMachine,
            ResourceConsumer::Process { pid },
            WrappedMeasurementValue::F64(value),
        )
    }

    // define the checks that you want to apply
    let runtime = RuntimeExpectations::new()
        // we need some metrics to create test data points
        .create_metric::<u64>("rapl_consumed_energy", Unit::Joule)
        .create_metric::<f64>("cpu_usage_percent", Unit::Unity)
        // not enough data at first
        .test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:00.00Z", 0));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:02.00Z", 100));

                    // cpu usage (per-consumer metric)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:02.00Z", 1, 50.0));
                }
                buf
            },
            |output| {
                /*
                Data received so far:
                - | time | energy | usage(1) | usage(2) |
                - |   00 | 0      |    50%   |          |
                - |   02 | 100    |          |          |

                Expected attribution:
                - nothing (not enough data)

                Expected buffer content:
                - same points as the input
                 */
                let metrics = TestMetrics::find_in(output.metrics());
                let m = output.measurements().to_vec();
                assert_eq!(
                    m,
                    vec![
                        new_point_energy(&metrics, "2025-05-02 00:00:00.00Z", 0),
                        new_point_energy(&metrics, "2025-05-02 00:00:02.00Z", 100),
                        new_point_usage(&metrics, "2025-05-02 00:00:02.00Z", 1, 50.0)
                    ]
                );
            },
        )
        .test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:04.00Z", 100));

                    // cpu usage (per-consumer metric)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:03.00Z", 1, 100.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:05.00Z", 1, 0.0));
                }
                buf
            },
            |output| {
                /*
                Data received so far:
                - | time | energy | usage(1) | usage(2) |
                - |   00 |    0   |     -    |          |
                - |   01 |    -   |     -    |          |
                - |   02 |  100   |    50%   |          |
                - |   03 |    -   |   100%   |          |
                - |   04 |  100   |     -    |          |
                - |   05 |    -   |     0%   |          |

                Expected attribution:
                - | time | attributed_energy |
                - |   00 |         -         |
                - |   01 |         -         |
                - |   02 |         -         |
                - |   03 |        100        |
                - |   04 |                   |
                - |   05 |                   |
                 */

                let metrics = TestMetrics::find_in(output.metrics());
                let data: Vec<_> = output.measurements().into_iter().collect();
                assert_eq!(data.len(), 3);

                assert_eq!(data[0].metric, metrics.rapl_consumed_energy);
                assert_eq!(data[0].value.as_u64(), 100);

                assert_eq!(data[1].metric, metrics.cpu_usage_percent);
                assert_eq!(data[1].value.as_f64(), 100.0);

                assert_eq!(data[2].metric, metrics.cpu_usage_percent);
                assert_eq!(data[2].value.as_f64(), 0.0);

                println!("-----");
                for m in output.measurements().iter() {
                    if m.metric == metrics.attributed_energy {
                        println!("{m:?}");
                    }
                }
                println!("-----");
            },
        )
        .test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:06.00Z", 500));

                    // cpu usage (per-consumer metric)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:06.00Z", 1, 90.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:06.00Z", 2, 10.0));
                }
                buf
            },
            |output| {
                let metrics = TestMetrics::find_in(output.metrics());
                println!("-----");
                for m in output.measurements().iter() {
                    if m.metric == metrics.attributed_energy {
                        println!("{m:?}");
                    }
                }
                println!("-----");
            },
        )
        .test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // ==== cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.00Z", 500));
                    // increase frequency to 10Hz
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.10Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.20Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.30Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.40Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.50Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.60Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.70Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.80Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:07.90Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:08.00Z", 500));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:09.00Z", 500));
                    // decrease frequency back to 1Hz
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:10.00Z", 500));

                    // ==== cpu usage (per-consumer metric)
                    // process 1: stay at 1Hz
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.00Z", 1, 90.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:08.00Z", 1, 1.0));

                    // process 2: 10Hz, gap, 1Hz
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.00Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.10Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.20Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.30Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.40Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.50Z", 2, 10.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:07.60Z", 2, 10.0));
                    // small gap
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:08.00Z", 2, 10.0));
                }
                buf
            },
            |output| {
                println!("-----");
                for m in output.measurements().iter() {
                    println!("{m:?}");
                }
                println!("-----");
            },
        );

    // start an Alumet agent
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<EnergyAttributionPlugin>(),
        enabled: true,
        config: Some(toml::from_str(CONFIG_CPU).unwrap()),
    });

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime) // load the checks
        .build_and_start()
        .unwrap();

    // wait for the agent to stop (it is automatically stopped by RuntimeExpectations)
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

fn init_logger() {
    // Ignore errors because the logger can only be initialized once, and we run multiple tests.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
}

/// Parses an RFC 3339 date-and-time string into a Timestamp value.
pub(crate) fn timestamp_from_rfc3339(timestamp: &str) -> Timestamp {
    SystemTime::from(OffsetDateTime::parse(timestamp, &Rfc3339).unwrap()).into()
}

struct TestMetrics {
    rapl_consumed_energy: RawMetricId,
    cpu_usage_percent: RawMetricId,
    attributed_energy: RawMetricId,
}

impl TestMetrics {
    fn find_in(metrics: &MetricRegistry) -> Self {
        let rapl_consumed_energy = metrics.by_name("rapl_consumed_energy").unwrap().0;
        let cpu_usage_percent = metrics.by_name("cpu_usage_percent").unwrap().0;
        let attributed_energy = metrics.by_name("attributed_energy").unwrap().0;
        Self {
            rapl_consumed_energy,
            cpu_usage_percent,
            attributed_energy,
        }
    }
}
