use alumet::{pipeline::Source, plugin::rust::AlumetPlugin, units::Unit};

use crate::{consistency::check_domains_consistency, perf_event::PerfEventProbe, powercap::PowercapProbe};

mod consistency;
mod cpus;
mod domains;
mod perf_event;
mod powercap;

pub struct RaplPlugin;

impl AlumetPlugin for RaplPlugin {
    fn name() -> &'static str {
        "rapl"
    }

    fn version() -> &'static str {
        "0.1.0"
    }
    
    fn init(_config: &mut alumet::config::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(RaplPlugin))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        // get cpu info, accessible perf events and power zones
        let all_cpus = cpus::online_cpus()?;
        let socket_cpus = cpus::cpus_to_monitor()?;
        let perf_events = perf_event::all_power_events()?;
        let power_zones = powercap::all_power_zones()?;

        let n_sockets = socket_cpus.len();
        let n_cpu_cores = all_cpus.len();
        log::debug!("{n_sockets}/{n_cpu_cores} monitorable CPU (cores) found: {socket_cpus:?}");

        let available_domains = check_domains_consistency(perf_events, power_zones);
        let subset_indicator = if available_domains.is_whole { "" } else { "(\"safe\" subset)" };
        log::info!("Available RAPL domains {subset_indicator}: {}", consistency::mkstring(&available_domains.domains, ", "));

        // create the probe
        let metric = alumet.create_metric::<f64>("rapl_consumed_energy", Unit::Joule, "Energy consumed since the previous measurement, as reported by RAPL.")?;
        let mut events_on_cpus = Vec::new();
        for event in &available_domains.perf_events {
            for cpu in &socket_cpus {
                events_on_cpus.push((event, cpu));
            }
        }
        log::debug!("Events to read: {events_on_cpus:?}");
        let source: Box<dyn Source> = match PerfEventProbe::new(metric, &events_on_cpus) {
            Ok(perf_event_probe) => Box::new(perf_event_probe),
            Err(_) => {
                // perf_events failed, log an error and try powercap instead
                log::error!("I could not use perf_events to read RAPL energy counters.");
                // TODO print some hints about permissions, setcap, sysctl -w perf_event_paranoid
                // TODO print how to configure alumet to disable this error.
                Box::new(PowercapProbe::new(metric, &available_domains.power_zones)?)
            },
        };
        alumet.add_source(source);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
