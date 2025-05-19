use std::time::Duration;

use alumet::{
    pipeline::elements::source::{trigger, Source},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};

use crate::{
    consistency::{get_available_domains, SafeSubset},
    perf_event::{PerfEventProbe, PowerEvent},
    powercap::{PowerZone, PowercapProbe},
};

#[cfg(test)]
use std::path::PathBuf;

mod consistency;
mod cpus;
mod domains;
pub mod perf_event;
mod powercap;

#[cfg(test)]
pub mod tests_mock;

pub struct RaplPlugin {
    config: Config,
}

impl RaplPlugin {
    #[cfg(not(test))]
    fn get_all_power_events(&self) -> anyhow::Result<Vec<PowerEvent>> {
        perf_event::all_power_events()
    }

    #[cfg(test)]
    fn get_all_power_events(&self) -> anyhow::Result<Vec<PowerEvent>> {
        perf_event::all_power_events_from_path(&self.config.perf_event_test_path)
    }

    #[cfg(not(test))]
    fn get_all_power_zones(&self) -> anyhow::Result<Vec<PowerZone>> {
        Ok(powercap::all_power_zones()?.flat)
    }

    #[cfg(test)]
    fn get_all_power_zones(&self) -> anyhow::Result<Vec<PowerZone>> {
        Ok(powercap::all_power_zones_from_path(&self.config.powercap_test_path)?.flat)
    }
}

impl AlumetPlugin for RaplPlugin {
    fn name() -> &'static str {
        "rapl"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(RaplPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let mut use_perf = !self.config.no_perf_events;
        let mut use_powercap = true;
        let mut check_consistency = true;

        if let Ok(false) = std::path::Path::new(perf_event::PERF_SYSFS_DIR).try_exists() {
            // PERF_SYSFS_DIR does not exist
            check_consistency = false;
            if use_perf {
                log::error!(
                    "{} does not exist, the Intel RAPL PMU module may not be enabled. Is your Linux kernel too old?",
                    perf_event::PERF_SYSFS_DIR
                );
                log::warn!("Because of the previous error, I will disable perf_events and fall back to powercap.");
                use_perf = false;
            } else {
                log::warn!(
                    "{} does not exist, the Intel RAPL PMU module may not be enabled. Is your Linux kernel too old?",
                    perf_event::PERF_SYSFS_DIR
                );
                log::warn!("I will not use perf_events to check the consistency of the RAPL interfaces.");
            }
        }

        // Discover RAPL domains available in perf_events and powercap. Beware, this can fail!
        let try_perf_events = self.get_all_power_events();
        let try_power_zones = self.get_all_power_zones();

        let (available_domains, subset_indicator) = get_available_domains(
            try_perf_events,
            try_power_zones,
            check_consistency,
            &mut use_perf,
            &mut use_powercap,
        )?;

        // We have found a set of RAPL domains that we agree on (in the best case, perf_events and powercap both work, are accessible by the agent and report the same list of domains).
        log::info!(
            "Available RAPL domains{subset_indicator}: {}",
            consistency::mkstring(&available_domains.domains, ", ")
        );

        // Create the metric.
        let metric = alumet.create_metric::<f64>(
            "rapl_consumed_energy",
            Unit::Joule,
            "Energy consumed since the previous measurement, as reported by RAPL.",
        )?;

        // Create the measurement source.
        let source = match (use_perf, use_powercap) {
            (true, true) => {
                // prefer perf_events, fallback to powercap if it fails
                setup_perf_events_probe_or_fallback(metric, &available_domains)?
            }
            (true, false) => {
                // only use perf
                Box::new(
                    PerfEventProbe::new(metric, &available_domains.perf_events)
                        .context("Failed to create RAPL probe based on perf_events")?,
                )
            }
            (false, true) => {
                // only use powercap
                Box::new(
                    PowercapProbe::new(metric, &available_domains.power_zones)
                        .context("Failed to create RAPL probe based on powercap")?,
                )
            }
            (false, false) => {
                // error: no available interface!
                return Err(anyhow!(
                    "I can use neither perf_events nor powercap: impossible to measure RAPL counters."
                ));
            }
        };

        // Configure the source and add it to Alumet
        let trigger = trigger::builder::time_interval(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .update_interval(self.config.flush_interval)
            .build()
            .unwrap();
        alumet.add_source("in", source, trigger)?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn setup_perf_events_probe_or_fallback(
    metric: alumet::metrics::TypedMetricId<f64>,
    available_domains: &SafeSubset,
) -> anyhow::Result<Box<dyn Source>> {
    match PerfEventProbe::new(metric, &available_domains.perf_events) {
        Ok(probe) => Ok(Box::new(probe)),
        Err(_) => {
            log::warn!(
                "I will fallback to the powercap sysfs, but perf_events is more efficient (see https://hal.science/hal-04420527)."
            );
            let fallback = PowercapProbe::new(metric, &available_domains.power_zones)?;
            Ok(Box::new(fallback))
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Initial interval between two RAPL measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Initial interval between two flushing of RAPL measurements.
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,

    /// Set to true to disable perf_events and always use the powercap sysfs.
    pub no_perf_events: bool,

    #[cfg(test)]
    pub perf_event_test_path: PathBuf,
    #[cfg(test)]
    pub powercap_test_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            no_perf_events: false, // prefer perf_events

            #[cfg(test)]
            perf_event_test_path: PathBuf::from(""),
            #[cfg(test)]
            powercap_test_path: PathBuf::from(""),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use crate::tests_mock::{create_mock_layout, create_valid_powercap_mock, Entry, EntryType};
    use crate::{Config, RaplPlugin};
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        measurement::{AttributeValue, WrappedMeasurementValue},
        pipeline::naming::SourceName,
        plugin::PluginMetadata,
        test::{RuntimeExpectations, StartupExpectations},
        units::Unit,
    };
    use tempfile::tempdir;

    /// This test ensure the plugin startup correctly, with the expected source based on Powercap Mocks created during the test.
    /// It also verifies the registered metrics and their units.
    #[test]
    fn test_startup_with_powercap() -> anyhow::Result<()> {
        let mut plugins = PluginSet::new();

        let base_path = create_valid_powercap_mock()?;

        let source_config = Config {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(1),
            no_perf_events: true,
            perf_event_test_path: Path::new("").to_path_buf(),
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

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectations)
            .build_and_start()
            .unwrap();

        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

        Ok(())
    }

    #[test]
    fn test_runtime_with_powercap() -> anyhow::Result<()> {
        let mut plugins = PluginSet::new();

        let tmp = tempdir()?;
        let base_path = tmp.keep();

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

        create_mock_layout(base_path.clone(), &entries)?;

        let source_config = Config {
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(1),
            no_perf_events: true,
            perf_event_test_path: Path::new("").to_path_buf(),
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
                |m| {
                    //note: it's expected to have no measurement as at first call of poll, cause the counter diff will return a None value
                    assert_eq!(m.len(), 0);
                },
            )
            .test_source(
                SourceName::from_str("rapl", "in"),
                || (),
                move |m| {
                    //note: the mock created 1 domain so it's expected to have 1 measurements
                    assert_eq!(m.len(), 1);
                    let mut actual_domains = Vec::new();
                    for measurement in m.iter() {
                        let attributes: Vec<_> = measurement.attributes().collect();
                        assert_eq!(attributes.len(), 1, "expected only one attribute 'domain'");
                        let domain_attribute = attributes[0];
                        assert_eq!(
                            domain_attribute.0, "domain",
                            "expected the attribute to have 'domain' key"
                        );
                        if let AttributeValue::String(domain) = domain_attribute.1 {
                            actual_domains.push(domain.clone());
                        } else {
                            assert!(false, "domain attribute should be of string type");
                        }
                        // I expect all the value to be 0 since the mock didn't change between the two poll runs
                        assert_eq!(measurement.value, WrappedMeasurementValue::F64(0.0));
                    }
                    let mut expected_domains = Vec::new();
                    expected_domains.push("package".to_string());

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
                    let _ = create_mock_layout(base_path.clone(), &entries);
                },
            )
            .test_source(
                SourceName::from_str("rapl", "in"),
                || (),
                |m| {
                    assert_eq!(m.len(), 1);
                    let measurement = m.iter().next().unwrap();

                    // expect to have an increase of 30000.0 Joules between the last two polls
                    assert_eq!(measurement.value, WrappedMeasurementValue::F64(30000.0));
                },
            );

        let agent = agent::Builder::new(plugins)
            .with_expectations(runtime_expectations)
            .build_and_start()
            .unwrap();

        agent.wait_for_shutdown(Duration::from_secs(10)).unwrap();

        Ok(())
    }

    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }
}
