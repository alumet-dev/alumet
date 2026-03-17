//! Integration tests for the energy to carbon transform.

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
    units::{Unit, PrefixedUnit},
};
use plugin_energy_to_carbon::EnergyToCarbonPlugin;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use pretty_assertions::assert_eq;

const TIMEOUT: Duration = Duration::from_secs(2);
const CONFIG_COUNTRY: &str = r#"
        # Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
        poll_interval = "2s"
        # "country", "override" or "world_avg"
        mode = "country"

        [country]
        # Country 3-letter ISO Code
        code = "FRA"

        [override]
        # Override the emission intensity value (in gCO₂/kWh).
        intensity = 100
"#;

// const CONFIG_OVERRIDE: &str = r#"
//         [plugins.energy-to-carbon]
//         # Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
//         poll_interval = "2s"
//         # "country", "override" or "world_avg"
//         mode = "override"

//         [plugins.energy-to-carbon.country]
//         # Country 3-letter ISO Code
//         code = "FRA"

//         [plugins.energy-to-carbon.override]
//         # Override the emission intensity value (in gCO₂/kWh).
//         intensity = 100
//     "#;

// const CONFIG_WORLD_AVG: &str = r#"
//         [plugins.energy-to-carbon]
//         # Time between each activation of the energy source (e.g. "1s", "500ms", "2m")
//         poll_interval = "2s"
//         # "country", "override" or "world_avg"
//         mode = "world_avg"

//         [plugins.energy-to-carbon.country]
//         # Country 3-letter ISO Code
//         code = "FRA"

//         [plugins.energy-to-carbon.override]
//         # Override the emission intensity value (in gCO₂/kWh).
//         intensity = 100
//     "#;

#[test]
fn test_energy_to_carbon() {
    init_logger();
    let attribution_transform = TransformName::from_str("energy-to-carbon", "transform");


    // Define input points
    fn new_point_energy(metrics: &TestMetrics, timestamp: &str, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            metrics.rapl_consumed_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(value),
        )
        .with_attr("domain", "package_total")
    }

    fn new_point_energy_prefixed(metrics: &TestMetrics, timestamp: &str, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            metrics.rapl_consumed_energy_prefixed,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(value),
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
        .with_attr("kind", "total")
    }

    // Define output points
    fn new_point_carbon(metrics: &TestMetrics, timestamp: &str, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            metrics.carbon_emission,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(value),
        )
        .with_attr("domain", "package_total")
        .with_attr("kind", "total")
    }



    // define the checks that you want to apply
    let runtime = RuntimeExpectations::new()
        // we need some metrics to create test data points
        .create_metric::<u64>("rapl_consumed_energy", Unit::Joule)
        .create_metric::<u64>("rapl_consumed_energy_prefixed", PrefixedUnit::milli(Unit::Joule))
        .create_metric::<f64>("cpu_usage_percent", Unit::Unity)
        
        // #### Basic RAPL energy transform ####
        .test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:01.00Z", 0.0));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:02.00Z", 100.0));

                    // cpu usage (we ignore these)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:01.00Z", 1, 50.0));
                }
                buf
            }, 
            |output| {
                /*
                Data received so far:
                - | time | energy   | usage(1) | usage(2) | carbon_emission 
                - |   00 |          |          |          |
                - |   01 | 0.0 J    |    50%   |          |  0   * 56.039 = 0.0
                - |   02 | 100.0 J  |          |          |  100 * 56.039 = 5 603.9
                 */
                let metrics = TestMetrics::find_in(output.metrics());
                let (input_measurements, new_measurements): (Vec<_>, Vec<_>) =
                    output.measurements().into_iter().cloned().partition(|p| {
                        p.metric == metrics.cpu_usage_percent || p.metric == metrics.rapl_consumed_energy
                    });

                    assert_eq!(
                    input_measurements,
                    vec![
                        new_point_usage(&metrics, "2025-05-02 00:00:01.00Z", 1, 50.0), 
                        new_point_energy(&metrics, "2025-05-02 00:00:02.00Z", 100.0),
                    ],
                    "input measurements should not be modified by energy-to-carbon"
                );
                
                assert_eq!(
                    new_measurements,
                    vec![
                        new_point_carbon(&metrics, "2025-05-02 00:00:01.00Z", 0.0),
                        new_point_carbon(&metrics, "2025-05-02 00:00:02.00Z", 5603.9),
                    ],
                    "incorrect transform result"
                );
            },
        // #### Adding multiples points at different timestamps ####
        ).test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:00.00Z", 50.0));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:01.00Z", 0.0));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:02.00Z", 100.0));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:03.00Z", 200.12));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:05.00Z", 0.0));

                    // cpu usage (we ignore these)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:01.00Z", 1, 50.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:03.00Z", 1, 80.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:05.00Z", 1, 20.0));
                }
                buf
            },
            |output| {
                /*
                Data received so far:
                - | time | energy   | usage(1) | usage(2) |      carbon_emission 
                - |   00 | 50.0 J    |          |          |  50     * 56.039 = 2 801.95
                - |   01 | 0.0 J     |    50%   |          |  0      * 56.039 = 0.0
                - |   02 | 100.0 J   |          |          |  100    * 56.039 = 5 603.9
                - |   03 | 200.12 J  |    80%   |          |  200.12 * 56.039 = 11 214.52468
                - |   04 |           |          |          |
                - |   05 | 0.0 J     |    20%   |          |  0      * 56.039 = 0.0
                 */
                let metrics = TestMetrics::find_in(output.metrics());
                let (input_measurements, new_measurements): (Vec<_>, Vec<_>) =
                    output.measurements().into_iter().cloned().partition(|p| {
                        p.metric == metrics.cpu_usage_percent || p.metric == metrics.rapl_consumed_energy
                    });

                assert_eq!(
                    new_measurements,
                    vec![
                        new_point_carbon(&metrics, "2025-05-02 00:00:00.00Z", 2801.95),
                        new_point_carbon(&metrics, "2025-05-02 00:00:01.00Z", 0.0),
                        new_point_carbon(&metrics, "2025-05-02 00:00:02.00Z", 5603.9),
                        new_point_carbon(&metrics, "2025-05-02 00:00:03.00Z", 11214.52468),
                        new_point_carbon(&metrics, "2025-05-02 00:00:05.00Z", 0.0),
                    ],
                    "incorrect transform result"
                );
            },
        // #### Adding prefixed Units ####
        ).test_transform(
            attribution_transform.clone(),
            |input| {
                let metrics = TestMetrics::find_in(input.metrics());
                let mut buf = MeasurementBuffer::new();
                {
                    // cpu energy (reference and global metric (per-resource with resource = LocalMachine))
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:01.00Z", 0.0));
                    buf.push(new_point_energy_prefixed(&metrics, "2025-05-02 00:00:02.00Z", 2000.5));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:03.00Z", 132.456));
                    buf.push(new_point_energy(&metrics, "2025-05-02 00:00:05.00Z", 0.0));

                    // cpu usage (we ignore these)
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:00.00Z", 1, 50.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:03.00Z", 1, 80.0));
                    buf.push(new_point_usage(&metrics, "2025-05-02 00:00:05.00Z", 1, 20.0));
                }
                buf
            },
            |output| {
                /*
                Data received so far:
                - | time | energy      | usage(1) | usage(2) |      carbon_emission 
                - |   00 |             |    50%   |          | 
                - |   01 | 0.0         |          |          |  0               * 56.039 = 0.0
                - |   02 | 2 000.5 mJ  |          |          |  (2 000.5/1 000) * 56.039 = 112.1060195
                - |   03 | 132.456 J   |    80%   |          |  132.456         * 56.039 = 7 422.701784
                - |   04 |             |          |          |
                - |   05 | 0.0         |    20%   |          |  0               * 56.039 = 0.0
                 */
                let metrics = TestMetrics::find_in(output.metrics());
                let (input_measurements, new_measurements): (Vec<_>, Vec<_>) =
                    output.measurements().into_iter().cloned().partition(|p| {
                        p.metric == metrics.cpu_usage_percent || p.metric == metrics.rapl_consumed_energy
                    });

                assert_eq!(
                    new_measurements,
                    vec![
                        new_point_carbon(&metrics, "2025-05-02 00:00:01.00Z", 0.0),
                        new_point_carbon(&metrics, "2025-05-02 00:00:02.00Z", 112.1060195),
                        new_point_carbon(&metrics, "2025-05-02 00:00:03.00Z", 7422.701784),
                        new_point_carbon(&metrics, "2025-05-02 00:00:05.00Z", 0.0),
                    ],
                    "incorrect transform result"
                );
            },
        );

        // start an Alumet agent
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<EnergyToCarbonPlugin>(),
        enabled: true,
        config: Some(toml::from_str(CONFIG_COUNTRY).unwrap()),
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
    rapl_consumed_energy_prefixed: RawMetricId,
    cpu_usage_percent: RawMetricId,
    carbon_emission: RawMetricId,
}

impl TestMetrics {
    fn find_in(metrics: &MetricRegistry) -> Self {
        let rapl_consumed_energy = metrics.by_name("rapl_consumed_energy").unwrap().0;
        let rapl_consumed_energy_prefixed = metrics.by_name("rapl_consumed_energy_prefixed").unwrap().0;
        let cpu_usage_percent = metrics.by_name("cpu_usage_percent").unwrap().0;
        let carbon_emission = metrics.by_name("carbon_emission").unwrap().0;
        Self {
            rapl_consumed_energy,
            rapl_consumed_energy_prefixed,
            cpu_usage_percent,
            carbon_emission,
        }
    }
}
