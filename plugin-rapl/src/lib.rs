use std::{default, time::Duration};

use alumet::{
    pipeline::{trigger, Source},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};
use anyhow::Context;
use indoc::indoc;
use serde::{Deserialize, Serialize};

use crate::{
    consistency::{check_domains_consistency, SafeSubset},
    perf_event::PerfEventProbe,
    powercap::PowercapProbe,
};

mod consistency;
mod cpus;
mod domains;
mod perf_event;
mod powercap;

pub struct RaplPlugin {
    config: Config,
}

impl AlumetPlugin for RaplPlugin {
    fn name() -> &'static str {
        "rapl"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(RaplPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        // Get cpu info.
        let all_cpus = cpus::online_cpus()?;
        let socket_cpus = cpus::cpus_to_monitor()?;
        let n_sockets = socket_cpus.len();
        let n_cpu_cores = all_cpus.len();
        log::debug!("{n_sockets}/{n_cpu_cores} monitorable CPU (cores) found: {socket_cpus:?}");

        // Discover RAPL domains available in perf_events and powercap.
        let perf_events = perf_event::all_power_events()?;
        let (available_domains, subset_indicator) = match powercap::all_power_zones() {
            Ok(power_zones) => {
                let domains = check_domains_consistency(perf_events, power_zones);
                let subset_indicator = if domains.is_whole { "" } else { " (\"safe\" subset)" };
                (domains, subset_indicator)
            }
            Err(e) => {
                log::warn!("The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines): {e}");
                let domains = SafeSubset::from_perf_only(perf_events);
                let subset_indicator = " (unchecked consistency)";
                (domains, subset_indicator)
            }
        };
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
        let mut events_on_cpus = Vec::new();
        for event in &available_domains.perf_events {
            for cpu in &socket_cpus {
                events_on_cpus.push((event, cpu));
            }
        }
        log::debug!("Events to read: {events_on_cpus:?}");
        let source = if self.config.no_perf_events {
            // perf_events disabled by config, use powercap directly
            setup_powercap_probe(metric, &available_domains)
        } else {
            // perf_events enabled, try it first and fallback to powercap if it fails
            setup_perf_events_probe(metric, events_on_cpus, &available_domains)
        };

        let trigger = trigger::builder::time_interval(self.config.poll_interval)
            .flush_interval(self.config.flush_interval)
            .update_interval(self.config.flush_interval)
            .build()
            .unwrap();
        alumet.add_source(source?, trigger);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn setup_perf_events_probe(
    metric: alumet::metrics::TypedMetricId<f64>,
    events_on_cpus: Vec<(&perf_event::PowerEvent, &cpus::CpuId)>,
    available_domains: &SafeSubset,
) -> Result<Box<dyn Source>, anyhow::Error> {
    let source: anyhow::Result<Box<dyn Source>> = match PerfEventProbe::new(metric, &events_on_cpus) {
        Ok(perf_event_probe) => Ok(Box::new(perf_event_probe)),
        Err(e) => {
            // perf_events failed, log an error and try powercap instead
            log::warn!("I could not use perf_events to read RAPL energy counters: {e}");
            let msg = indoc! {"
                    I will fallback to the powercap sysfs, but perf_events is more efficient (see https://hal.science/hal-04420527).
                    
                    This warning is probably caused by insufficient privileges.
                    To fix this, you have 3 possibilities:
                    1. Grant the CAP_PERFMON (CAP_SYS_ADMIN on Linux < 5.8) capability to the agent binary.
                        sudo setcap cap_perfmon=ep $(readlink -f path/to/alumet-agent)
                    
                    2. Change a kernel setting to allow every process to read the perf_events.
                        sudo sysctl -w kernel.perf_event_paranoid=0
                    
                    3. Run the agent as root (not recommanded).
                "};
            log::warn!("{msg}");

            setup_powercap_probe(metric, available_domains)
        }
    };
    source
}

fn setup_powercap_probe(
    metric: alumet::metrics::TypedMetricId<f64>,
    available_domains: &SafeSubset,
) -> anyhow::Result<Box<dyn Source>> {
    match PowercapProbe::new(metric, &available_domains.power_zones) {
        Ok(powercap_probe) => Ok(Box::new(powercap_probe)),
        Err(e) => {
            let msg = indoc! {"
                I could not use the powercap sysfs to read RAPL energy counters.
                This is probably caused by insufficient privileges.
                Please check that you have read access to everything in '/sys/devices/virtual/powercap/intel-rapl'.
                    
                A solution could be:
                    sudo chmod a+r -R /sys/devices/virtual/powercap/intel-rapl
            "};
            log::error!("{msg}");
            Err(e)
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Config {
    /// Initial interval between two RAPL measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// Initial interval between two flushing of RAPL measurements.
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,

    /// Set to true to disable perf_events and always use the powercap sysfs.
    no_perf_events: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            no_perf_events: false, // prefer perf_events
        }
    }
}
