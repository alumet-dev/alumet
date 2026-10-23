use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::MeasurementPoint,
    pipeline::naming::SourceName,
    plugin::{PluginMetadata, rust::deserialize_config},
    resources::Resource,
    test::{RuntimeExpectations, StartupExpectations},
    units::Unit,
};
use plugin_infiniband::{Config, InfinibandPlugin};
use pretty_assertions::assert_eq;
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(10);
const METRIC_PACKETS: &str = "infiniband_packets_per_second";
const METRIC_BYTES: &str = "infiniband_bytes_per_second";

#[derive(Debug, Clone, Copy, Default)]
struct Counters {
    rcv_packets: u64,
    xmit_packets: u64,
    rcv_data: u64,
    xmit_data: u64,
}

fn create_port(root: &Path, device: &str, port: u32) -> PathBuf {
    let counters = root.join(device).join("ports").join(port.to_string()).join("counters");
    std::fs::create_dir_all(&counters).unwrap();
    counters
}

fn write_counters(counters_dir: &Path, suffix: &str, counters: Counters) {
    for (base, value) in [
        ("port_rcv_packets", counters.rcv_packets),
        ("port_xmit_packets", counters.xmit_packets),
        ("port_rcv_data", counters.rcv_data),
        ("port_xmit_data", counters.xmit_data),
    ] {
        std::fs::write(counters_dir.join(format!("{base}{suffix}")), format!("{value}\n")).unwrap();
    }
}

fn config(root: &Path, devices: Vec<String>, ports: Vec<u32>) -> Config {
    Config {
        poll_interval: Duration::from_secs(1),
        root_path: root.to_path_buf(),
        devices,
        ports,
    }
}

fn config_to_toml_table(config: &Config) -> toml::Table {
    toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
}

fn add_plugin(plugins: &mut PluginSet, config: &Config) {
    plugins.add_plugin(alumet::agent::plugin::PluginInfo {
        metadata: PluginMetadata::from_static::<InfinibandPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(config)),
    });
}

fn startup_expectations() -> StartupExpectations {
    StartupExpectations::new()
        .expect_metric::<f64>(METRIC_PACKETS, Unit::Hertz)
        .expect_metric::<f64>(METRIC_BYTES, Unit::Byte)
        .expect_source("infiniband", "ports")
}

fn get_point<'a>(
    points: &'a [&'a MeasurementPoint],
    metric: alumet::metrics::RawMetricId,
    resource: &Resource,
    direction: &str,
) -> &'a MeasurementPoint {
    points
        .iter()
        .copied()
        .find(|p| {
            p.metric == metric
                && &p.resource == resource
                && p.attributes()
                    .any(|(k, v)| k == "direction" && v.to_string() == direction)
        })
        .unwrap_or_else(|| panic!("no point for metric {metric:?}, resource {resource:?}, direction {direction}"))
}

fn assert_port_attributes(point: &MeasurementPoint, device: &str, port: u64) {
    let attrs: Vec<_> = point.attributes().collect();
    assert!(
        attrs.iter().any(|(k, v)| *k == "interface" && v.to_string() == device),
        "missing or wrong interface attribute: {attrs:?}"
    );
    assert!(
        attrs
            .iter()
            .any(|(k, v)| *k == "port" && v.to_string() == port.to_string()),
        "missing or wrong port attribute: {attrs:?}"
    );
}

#[test]
fn first_poll_has_no_measurements_then_rates_are_emitted() {
    let root = tempdir().unwrap();
    let counters_dir = create_port(root.path(), "mlx5_0", 1);
    write_counters(&counters_dir, "", Counters::default());

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec![], vec![]));

    let source_name = SourceName::from_str("infiniband", "ports");
    let second_counters = Counters {
        rcv_packets: 1_000_000,
        xmit_packets: 2_000_000,
        rcv_data: 250,
        xmit_data: 500,
    };
    let update_dir = counters_dir.clone();
    let runtime = RuntimeExpectations::new()
        .test_source(
            source_name.clone(),
            || {},
            |ctx| {
                assert!(
                    ctx.measurements().is_empty(),
                    "the first poll should only establish a baseline"
                );
                // Let enough time pass so that the second poll has a positive elapsed time.
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name,
            move || write_counters(&update_dir, "", second_counters),
            |ctx| {
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(points.len(), 4, "one point per metric and direction");

                let packets = ctx.metrics().by_name(METRIC_PACKETS).unwrap().0;
                let bytes = ctx.metrics().by_name(METRIC_BYTES).unwrap().0;
                let resource = Resource::custom("infiniband_port", "mlx5_0:1");

                let rx_packets = get_point(&points, packets, &resource, "rx");
                let tx_packets = get_point(&points, packets, &resource, "tx");
                let rx_bytes = get_point(&points, bytes, &resource, "rx");
                let tx_bytes = get_point(&points, bytes, &resource, "tx");

                for point in &points {
                    assert_port_attributes(point, "mlx5_0", 1);
                }

                // 250 data units * 4 bytes/unit / 1_000_000 packets = 0.001 B/packet.
                // This ratio proves that data counters are multiplied by 4.
                let rx_ratio = rx_bytes.value.as_f64() / rx_packets.value.as_f64();
                assert!(
                    (rx_ratio - 0.001).abs() < 1e-12,
                    "ratios: rx_bytes={}, rx_packets={}, ratio={rx_ratio}",
                    rx_bytes.value.as_f64(),
                    rx_packets.value.as_f64()
                );
                let tx_ratio = tx_bytes.value.as_f64() / tx_packets.value.as_f64();
                assert!(
                    (tx_ratio - 0.001).abs() < 1e-12,
                    "ratios: tx_bytes={}, tx_packets={}, ratio={tx_ratio}",
                    tx_bytes.value.as_f64(),
                    tx_packets.value.as_f64()
                );
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn counter_reset_produces_no_measurement() {
    let root = tempdir().unwrap();
    let counters_dir = create_port(root.path(), "mlx5_0", 1);
    write_counters(
        &counters_dir,
        "",
        Counters {
            rcv_packets: 1000,
            xmit_packets: 1000,
            rcv_data: 1000,
            xmit_data: 1000,
        },
    );

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec![], vec![]));

    let source_name = SourceName::from_str("infiniband", "ports");
    let update_dir = counters_dir.clone();
    let rearm_dir = counters_dir.clone();
    let runtime = RuntimeExpectations::new()
        .test_source(
            source_name.clone(),
            || {},
            |ctx| {
                assert!(ctx.measurements().is_empty());
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name.clone(),
            move || {
                // Counters decreased: the baseline must be reset without emitting bogus rates.
                write_counters(
                    &update_dir,
                    "",
                    Counters {
                        rcv_packets: 1,
                        xmit_packets: 1,
                        rcv_data: 1,
                        xmit_data: 1,
                    },
                )
            },
            |ctx| {
                assert!(
                    ctx.measurements().is_empty(),
                    "a counter reset should produce no measurement"
                );
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name,
            move || {
                // Monotonic counters after the reset: the new baseline must be used.
                write_counters(
                    &rearm_dir,
                    "",
                    Counters {
                        rcv_packets: 1001,
                        xmit_packets: 1001,
                        rcv_data: 1001,
                        xmit_data: 1001,
                    },
                )
            },
            |ctx| {
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(
                    points.len(),
                    4,
                    "emission should resume from the new baseline after a reset"
                );
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn filters_restrict_devices_and_ports() {
    let root = tempdir().unwrap();
    let mlx5_0_port_1 = create_port(root.path(), "mlx5_0", 1);
    let mlx5_0_port_2 = create_port(root.path(), "mlx5_0", 2);
    let mlx5_1_port_1 = create_port(root.path(), "mlx5_1", 1);
    for dir in [&mlx5_0_port_1, &mlx5_0_port_2, &mlx5_1_port_1] {
        write_counters(dir, "", Counters::default());
    }

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec!["mlx5_0".to_string()], vec![1]));

    let source_name = SourceName::from_str("infiniband", "ports");
    let update_dirs = [mlx5_0_port_1, mlx5_0_port_2, mlx5_1_port_1];
    let runtime = RuntimeExpectations::new()
        .test_source(
            source_name.clone(),
            || {},
            |ctx| {
                assert!(ctx.measurements().is_empty());
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name,
            move || {
                for dir in &update_dirs {
                    write_counters(
                        dir,
                        "",
                        Counters {
                            rcv_packets: 100,
                            xmit_packets: 200,
                            rcv_data: 100,
                            xmit_data: 200,
                        },
                    );
                }
            },
            |ctx| {
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(points.len(), 4, "only the filtered port should be reported");

                let expected = Resource::custom("infiniband_port", "mlx5_0:1");
                for point in points {
                    assert_eq!(point.resource, expected);
                    assert_port_attributes(point, "mlx5_0", 1);
                }
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn prefers_64_bit_counters() {
    let root = tempdir().unwrap();
    let counters_dir = create_port(root.path(), "mlx5_0", 1);
    // Both variants exist. The plain variants will decrease on the second poll,
    // while the _64 variants increase. If the plugin reads the plain variants,
    // compute_rates returns None and no measurement is emitted.
    write_counters(
        &counters_dir,
        "",
        Counters {
            rcv_packets: 1000,
            xmit_packets: 1000,
            rcv_data: 1000,
            xmit_data: 1000,
        },
    );
    write_counters(
        &counters_dir,
        "_64",
        Counters {
            rcv_packets: 1000,
            xmit_packets: 1000,
            rcv_data: 1000,
            xmit_data: 1000,
        },
    );

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec![], vec![]));

    let source_name = SourceName::from_str("infiniband", "ports");
    let update_dir = counters_dir.clone();
    let runtime = RuntimeExpectations::new()
        .test_source(
            source_name.clone(),
            || {},
            |ctx| {
                assert!(ctx.measurements().is_empty());
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name,
            move || {
                write_counters(
                    &update_dir,
                    "",
                    Counters {
                        rcv_packets: 1,
                        xmit_packets: 1,
                        rcv_data: 1,
                        xmit_data: 1,
                    },
                );
                write_counters(
                    &update_dir,
                    "_64",
                    Counters {
                        rcv_packets: 2000,
                        xmit_packets: 3000,
                        rcv_data: 1250,
                        xmit_data: 1750,
                    },
                );
            },
            |ctx| {
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(points.len(), 4, "the _64 counters should be used");

                let packets = ctx.metrics().by_name(METRIC_PACKETS).unwrap().0;
                let bytes = ctx.metrics().by_name(METRIC_BYTES).unwrap().0;
                let resource = Resource::custom("infiniband_port", "mlx5_0:1");

                let rx_packets = get_point(&points, packets, &resource, "rx");
                let rx_bytes = get_point(&points, bytes, &resource, "rx");

                // _64 deltas: 1000 packets and 250 data units -> 1 B/packet (250 * 4 / 1000).
                let ratio = rx_bytes.value.as_f64() / rx_packets.value.as_f64();
                assert!(
                    (ratio - 1.0).abs() < 1e-12,
                    "unexpected ratio {ratio}: _64 counters were not used as expected"
                );
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
fn missing_root_fails_startup() {
    let root = tempdir().unwrap();
    let missing = root.path().join("does-not-exist");

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(&missing, vec![], vec![]));

    let err = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .build_and_start()
        .err()
        .expect("startup must fail on a host without an InfiniBand sysfs root");
    let message = format!("{err:#}");
    assert!(
        message.contains("cannot detect InfiniBand ports"),
        "unexpected error: {message}"
    );
}

#[test]
fn empty_root_fails_startup() {
    let root = tempdir().unwrap();

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec![], vec![]));

    let err = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .build_and_start()
        .err()
        .expect("startup must fail when no InfiniBand port is detected");
    let message = format!("{err:#}");
    assert!(
        message.contains("no InfiniBand port found"),
        "unexpected error: {message}"
    );
}

#[test]
fn port_appearing_after_startup_is_detected() {
    let root = tempdir().unwrap();
    let first = create_port(root.path(), "mlx5_0", 1);
    write_counters(&first, "", Counters::default());

    let mut plugins = PluginSet::new();
    add_plugin(&mut plugins, &config(root.path(), vec![], vec![]));

    let source_name = SourceName::from_str("infiniband", "ports");
    let root_dir = root.path().to_path_buf();
    let first_dir = first.clone();
    let runtime = RuntimeExpectations::new()
        .test_source(
            source_name.clone(),
            || {},
            |ctx| {
                assert!(ctx.measurements().is_empty(), "first poll is the baseline");
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name.clone(),
            move || {
                // A second port shows up after startup, and traffic starts on both.
                let second = create_port(&root_dir, "mlx5_1", 1);
                for dir in [&first_dir, &second] {
                    write_counters(
                        dir,
                        "",
                        Counters {
                            rcv_packets: 100,
                            xmit_packets: 100,
                            rcv_data: 100,
                            xmit_data: 100,
                        },
                    );
                }
            },
            |ctx| {
                // Only the port known at startup can be rated on this poll: the new
                // one is at its baseline.
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(points.len(), 4);
                for point in points {
                    assert_eq!(point.resource, Resource::custom("infiniband_port", "mlx5_0:1"));
                }
                std::thread::sleep(Duration::from_secs(1));
            },
        )
        .test_source(
            source_name,
            || {},
            |ctx| {
                let points: Vec<&MeasurementPoint> = ctx.measurements().into_iter().collect();
                assert_eq!(points.len(), 8, "the port that appeared later must be reported too");
                let resources: std::collections::HashSet<_> = points.iter().map(|p| p.resource.clone()).collect();
                let expected: std::collections::HashSet<_> = [
                    Resource::custom("infiniband_port", "mlx5_0:1"),
                    Resource::custom("infiniband_port", "mlx5_1:1"),
                ]
                .into_iter()
                .collect();
                assert_eq!(resources, expected);
            },
        );

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup_expectations())
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

// Ensure that the public Config can be deserialized, as documented.
#[test]
fn config_is_public_and_deserializable() {
    let config: Config = toml::from_str("poll_interval = \"2s\"").unwrap();
    assert_eq!(config.poll_interval, Duration::from_secs(2));
    let table = alumet::plugin::ConfigTable(toml::Value::try_from(config).unwrap().as_table().unwrap().clone());
    let _: Config = deserialize_config(table).unwrap();
}
