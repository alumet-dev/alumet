use std::{path::PathBuf, time::Duration};

use alumet::{
    pipeline::elements::source::{trigger, Source},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};
use anyhow::{anyhow, Context};
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
        let try_perf_events = perf_event::all_power_events();
        let try_power_zones = powercap::all_power_zones();

        let (available_domains, subset_indicator) = match (try_perf_events, try_power_zones) {
            (Ok(perf_events), Ok(power_zones)) => {
                if !check_consistency {
                    (SafeSubset::from_powercap_only(power_zones), " (from powercap)")
                } else {
                    let mut safe_domains = check_domains_consistency(&perf_events, &power_zones);
                    let mut domain_origin = "";
                    if !safe_domains.is_whole {
                        // If one of the domain set is smaller, it could be empty, which would prevent the plugin from measuring anything.
                        // In that case, we fall back to the other interface, the one that reports a non-empty list of domains.
                        if perf_events.is_empty() && !power_zones.top.is_empty() {
                            log::warn!("perf_events returned an empty list of RAPL domains, I will disable perf_events and use powercap instead.");
                            use_perf = false;
                            safe_domains = SafeSubset::from_powercap_only(power_zones);
                            domain_origin = " (from powercap)";
                        } else if !perf_events.is_empty() && power_zones.top.is_empty() {
                            log::warn!("perf_events returned an empty list of RAPL domains, I will disable powercap and use perf_events instead.");
                            use_powercap = false;
                            safe_domains = SafeSubset::from_perf_only(perf_events);
                            domain_origin = " (from perf_events)";
                        } else {
                            domain_origin = " (\"safe subset\")";
                        }
                    }
                    (safe_domains, domain_origin)
                }
            }
            (Ok(perf_events), Err(powercap_err)) => {
                log::error!(
                    "Cannot read the list of RAPL domains available via the powercap interface: {powercap_err:?}."
                );
                log::warn!("The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines).");
                (SafeSubset::from_perf_only(perf_events), " (from perf_events)")
            }
            (Err(perf_err), Ok(power_zones)) => {
                log::warn!(
                    "Cannot read the list of RAPL domains available via the perf_events interface: {perf_err:?}."
                );
                log::warn!("The consistency of the RAPL domains reported by the different interfaces of the Linux kernel cannot be checked (this is useful to work around bugs in some kernel versions on some machines).");
                (SafeSubset::from_powercap_only(power_zones), " (from powercap)")
            }
            (Err(perf_err), Err(power_err)) => {
                log::error!("I could use neither perf_events nor powercap.\nperf_events error: {perf_err:?}\npowercap error: {power_err:?}");
                Err(anyhow!(
                    "Both perf_events and powercap failed, unable to read RAPL counters: {perf_err}\n{power_err}"
                ))?
            }
        };

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
                setup_perf_events_probe(metric, &available_domains)
                    .context("Failed to create RAPL probe based on perf_events")?
            }
            (false, true) => {
                // only use powercap
                setup_powercap_probe(metric, &available_domains)
                    .context("Failed to create RAPL probe based on powercap")?
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
    setup_perf_events_probe(metric, available_domains).or_else(|_| {
        log::warn!("I will fallback to the powercap sysfs, but perf_events is more efficient (see https://hal.science/hal-04420527).");
        setup_powercap_probe(metric, available_domains)
    })
}

fn setup_perf_events_probe(
    metric: alumet::metrics::TypedMetricId<f64>,
    available_domains: &SafeSubset,
) -> Result<Box<dyn Source>, anyhow::Error> {
    fn resolve_application_path() -> std::io::Result<PathBuf> {
        std::env::current_exe()?.canonicalize()
    }

    // Get cpu info (this can fail in some weird circumstances, let's be robust).
    let all_cpus = cpus::online_cpus()?;
    let socket_cpus = cpus::cpus_to_monitor_with_perf()
        .context("I could not determine how to use perf_events to read RAPL energy counters. The Intel RAPL PMU module may not be enabled, is your Linux kernel too old?")?;

    let n_sockets = socket_cpus.len();
    let n_cpu_cores = all_cpus.len();
    log::debug!("{n_sockets}/{n_cpu_cores} monitorable CPU (cores) found: {socket_cpus:?}");

    // Build the right combination of perf events.
    let mut events_on_cpus = Vec::new();
    for event in &available_domains.perf_events {
        for cpu in &socket_cpus {
            events_on_cpus.push((event, cpu));
        }
    }
    log::debug!("Events to read: {events_on_cpus:?}");

    // Try to create the source
    match PerfEventProbe::new(metric, &events_on_cpus) {
        Ok(perf_event_probe) => Ok(Box::new(perf_event_probe)),
        Err(e) => {
            // perf_events failed, log an error and try powercap instead
            log::warn!("I could not use perf_events to read RAPL energy counters: {e}");
            let app_path = resolve_application_path()
                .ok()
                .and_then(|p| p.to_str().map(|s| s.to_owned()))
                .unwrap_or(String::from("path/to/agent"));
            let msg = indoc::formatdoc! {"
                    I will fallback to the powercap sysfs, but perf_events is more efficient (see https://hal.science/hal-04420527).
                    
                    This warning is probably caused by insufficient privileges.
                    To fix this, you have 3 possibilities:
                    1. Grant the CAP_PERFMON (CAP_SYS_ADMIN on Linux < 5.8) capability to the agent binary.
                         sudo setcap cap_perfmon=ep \"{app_path}\"
                        
                       Note: to grant multiple capabilities to the binary, you must put all the capabilities in the same command.
                         sudo setcap \"cap_sys_nice+ep cap_perfmon=ep\" \"{app_path}\" 
                    
                    2. Change a kernel setting to allow every process to read the perf_events.
                        sudo sysctl -w kernel.perf_event_paranoid=0
                    
                    3. Run the agent as root (not recommanded).
                "};
            log::warn!("{msg}");
            Err(e)
        }
    }
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
#[serde(deny_unknown_fields)]
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
            flush_interval: Duration::from_secs(1),
            no_perf_events: false, // prefer perf_events
        }
    }
}
