use std::time::Duration;

use alumet::{pipeline::{trigger::TriggerSpec, Source}, plugin::{rust::AlumetPlugin, ConfigTable}, units::Unit};
use indoc::indoc;

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
    poll_interval: Duration,
}

impl AlumetPlugin for RaplPlugin {
    fn name() -> &'static str {
        "rapl"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO read from config
        let poll_interval = Duration::from_secs(1);
        Ok(Box::new(RaplPlugin { poll_interval }))
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

        // Create the probe.
        let metric = alumet.create_metric::<f64>(
            "rapl_consumed_energy",
            Unit::Joule,
            "Energy consumed since the previous measurement, as reported by RAPL.",
        )?;
        let mut events_on_cpus = Vec::new();
        for event in &available_domains.perf_events {
            for cpu in &socket_cpus {
                events_on_cpus.push((event, cpu));
            }
        }
        log::debug!("Events to read: {events_on_cpus:?}");
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
                // TODO add an option to disable perf_events and always use sysfs.

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
        };
        alumet.add_source(source?, TriggerSpec::at_interval(self.poll_interval));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
