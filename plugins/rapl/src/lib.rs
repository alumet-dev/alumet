use std::{path::Path, time::Duration};

use alumet::{
    metrics::TypedMetricId,
    pipeline::elements::source::{Source, trigger::builder},
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    consistency::{SafeSubset, get_available_domains},
    perf_event::{PERF_SYSFS_DIR, PerfEventProbe, PowerEvent},
    powercap::{PowerZone, PowercapProbe},
};

#[cfg(test)]
use std::path::PathBuf;

mod consistency;
mod cpus;
mod domains;
mod perf_event;
mod powercap;
mod tests;
mod total;

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

    #[cfg(not(test))]
    fn perf_sysfs_dir(&self) -> &Path {
        Path::new(PERF_SYSFS_DIR)
    }

    #[cfg(test)]
    fn perf_sysfs_dir(&self) -> &Path {
        Path::new("/i/do/not/exists")
    }
}

fn setup_perf_events_probe_or_fallback(
    metric: TypedMetricId<f64>,
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

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let mut use_perf = !self.config.no_perf_events;
        let mut use_powercap = true;
        let mut check_consistency = true;

        if let Ok(false) = self.perf_sysfs_dir().try_exists() {
            // PERF_SYSFS_DIR does not exist
            check_consistency = false;
            if use_perf {
                log::error!(
                    "{} does not exist, the Intel RAPL PMU module may not be enabled. Is your Linux kernel too old?",
                    PERF_SYSFS_DIR
                );
                log::warn!("Because of the previous error, I will disable perf_events and fall back to powercap.");
                use_perf = false;
            } else {
                log::warn!(
                    "{} does not exist, the Intel RAPL PMU module may not be enabled. Is your Linux kernel too old?",
                    PERF_SYSFS_DIR
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
        let trigger = builder::time_interval(self.config.poll_interval)
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
mod test {
    use crate::{Config, RaplPlugin};
    use alumet::plugin::rust::{AlumetPlugin, deserialize_config};
    use std::time::Duration;

    #[test]
    fn test_default_config() {
        let table = RaplPlugin::default_config().expect("default_config() should not fail");
        let config: Config = deserialize_config(table.expect("default_config() should return Some")).unwrap();

        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.flush_interval, Duration::from_secs(5));
        assert_eq!(config.no_perf_events, false);
    }
}
