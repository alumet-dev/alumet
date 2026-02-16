use crate::tests::mocks::*;
use crate::{Config, RaplPlugin};
use alumet::{
    agent::{
        Builder,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{AttributeValue, WrappedMeasurementValue},
    pipeline::naming::SourceName,
    plugin::PluginMetadata,
    test::{RuntimeExpectations, StartupExpectations},
    units::Unit,
};
use std::{path::Path, thread::sleep, time::Duration};
use tempfile::tempdir;

use toml::{Table, Value};

#[cfg(test)]
fn config_to_toml_table(config: &Config) -> Table {
    Value::try_from(config).unwrap().as_table().unwrap().clone()
}

/// This test ensure the plugin startup correctly, with the expected source based on Powercap Mocks created during the test.
/// It also verifies the registered metrics and their units.
#[test]
fn test_startup_with_powercap() -> anyhow::Result<()> {
    let mut plugins = PluginSet::new();

    let tmp = create_valid_powercap_mock()?;
    let base_path = tmp.path().to_owned();
    let perf_event_test_path = Path::new("").to_path_buf();

    let source_config = Config {
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
        no_perf_events: true,
        perf_event_test_path,
        powercap_test_path: base_path,
    };
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<RaplPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&source_config)),
    });

    let startup_expectations = StartupExpectations::new()
        .expect_metric::<f64>("rapl_consumed_energy", Unit::Joule)
        .expect_source("rapl", "in");

    let agent = Builder::new(plugins)
        .with_expectations(startup_expectations)
        .build_and_start()
        .unwrap();

    sleep(Duration::from_millis(200)); // don't shutdown right away, else we get in trouble
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

    Ok(())
}

#[test]
fn test_runtime_with_powercap() -> anyhow::Result<()> {
    let mut plugins = PluginSet::new();

    let tmp = tempdir()?;
    let base_path = tmp.path().to_owned();
    let perf_event_test_path = Path::new("").to_path_buf();

    use EntryType::*;

    let entries = [
        Entry {
            path: "enabled",
            entry_type: File("1"),
        },
        Entry {
            path: "intel-rapl:0",
            entry_type: Dir,
        },
        Entry {
            path: "intel-rapl:0/name",
            entry_type: File("package-0"),
        },
        Entry {
            path: "intel-rapl:0/max_energy_range_uj",
            entry_type: File("262143328850"),
        },
        Entry {
            path: "intel-rapl:0/energy_uj",
            entry_type: File("124599532281"),
        },
    ];

    create_mock_layout(&base_path, &entries)?;

    let source_config = Config {
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
        no_perf_events: true,
        perf_event_test_path,
        powercap_test_path: base_path.clone(),
    };
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<RaplPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&source_config)),
    });

    let runtime_expectations = RuntimeExpectations::new()
        .test_source(
            SourceName::from_str("rapl", "in"),
            || (),
            |ctx| {
                //note: it's expected to have no measurement as at first call of poll, cause the counter diff will return a None value
                assert_eq!(ctx.measurements().len(), 0);
            },
        )
        .test_source(
            SourceName::from_str("rapl", "in"),
            || (),
            move |ctx| {
                // note: the mock created 1 domain so it's expected to have 2 measurements:
                // the domain's value, and one per-domain total
                let m = ctx.measurements();
                assert_eq!(m.len(), 2);
                let mut actual_domains = Vec::new();
                for measurement in m.iter() {
                    let attributes: Vec<_> = measurement.attributes().collect();
                    assert_eq!(attributes.len(), 1, "expected only one attribute 'domain'");
                    let domain_attribute = attributes[0];
                    assert_eq!(
                        domain_attribute.0, "domain",
                        "expected the attribute to have 'domain' key"
                    );
                    if let AttributeValue::Str(domain) = domain_attribute.1 {
                        actual_domains.push(domain);
                    } else {
                        assert!(false, "domain attribute should be of Str type");
                    }
                    // I expect all the value to be 0 since the mock didn't change between the two poll runs
                    assert_eq!(measurement.value, WrappedMeasurementValue::F64(0.0));
                }
                let mut expected_domains = Vec::from_iter(&["package", "package_total"]);

                actual_domains.sort();
                expected_domains.sort();
                assert_eq!(actual_domains, expected_domains);

                // creating new mocks to make values change for next poll
                let entries = [
                    Entry {
                        path: "enabled",
                        entry_type: File("1"),
                    },
                    Entry {
                        path: "intel-rapl:0",
                        entry_type: Dir,
                    },
                    Entry {
                        path: "intel-rapl:0/name",
                        entry_type: File("package-0"),
                    },
                    Entry {
                        path: "intel-rapl:0/max_energy_range_uj",
                        entry_type: File("262143328850"),
                    },
                    Entry {
                        path: "intel-rapl:0/energy_uj",
                        entry_type: File("154599532281"),
                    },
                ];
                let _ = create_mock_layout(&base_path, &entries);
            },
        )
        .test_source(
            SourceName::from_str("rapl", "in"),
            || (),
            |ctx| {
                let m = ctx.measurements();
                assert!(m.len() >= 1);
                let measurement = m.iter().next().unwrap();

                // expect to have an increase of 30000.0 Joules between the last two polls
                assert_eq!(measurement.value, WrappedMeasurementValue::F64(30000.0));
            },
        );

    let agent = Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

    Ok(())
}

#[test]
fn test_missing_disable_perf_events() -> anyhow::Result<()> {
    let mut plugins = PluginSet::new();

    let tmp = create_valid_powercap_mock()?;
    let base_path = tmp.path().to_owned();
    let perf_event_test_path = Path::new("").to_path_buf();

    let source_config = Config {
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
        no_perf_events: false, // perf activated at starting
        perf_event_test_path,
        powercap_test_path: base_path,
    };

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<RaplPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&source_config)),
    });

    let startup_expectations = StartupExpectations::new()
        .expect_metric::<f64>("rapl_consumed_energy", Unit::Joule)
        .expect_source("rapl", "in");

    let agent = Builder::new(plugins)
        .with_expectations(startup_expectations)
        .build_and_start()
        .unwrap();

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

    Ok(())
}

#[test]
fn test_powercap_probe_error_propagated() -> anyhow::Result<()> {
    let mut plugins = PluginSet::new();

    let tmp = tempdir()?;
    let base_path = tmp.path().to_owned();
    let perf_event_test_path = "/i/do/not/exists".into();

    // Missing max_energy_range_uj file
    let entries = [
        Entry {
            path: "intel-rapl:0",
            entry_type: EntryType::Dir,
        },
        Entry {
            path: "intel-rapl:0/name",
            entry_type: EntryType::File("package-0"),
        },
        Entry {
            path: "intel-rapl:0/energy_uj",
            entry_type: EntryType::File("10"),
        },
    ];

    create_mock_layout(&base_path, &entries)?;

    let config = Config {
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
        no_perf_events: true,
        perf_event_test_path,
        powercap_test_path: base_path,
    };

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<RaplPlugin>(),
        enabled: true,
        config: Some(config_to_toml_table(&config)),
    });

    let agent = Builder::new(plugins).build_and_start();
    assert!(
        agent.is_err(),
        "Expected PowercapProbe::new to fail and propagate error"
    );

    Ok(())
}
