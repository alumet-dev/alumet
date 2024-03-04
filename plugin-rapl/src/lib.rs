use alumet::{metrics::WrappedMeasurementType, units::Unit};

use crate::{consistency::check_domains_consistency, perf_event::PerfEventProbe};

mod consistency;
mod cpus;
mod domains;
mod perf_event;
mod powercap;

pub struct RaplPlugin;

impl alumet::plugin::Plugin for RaplPlugin {
    fn name(&self) -> &str {
        "rapl"
    }

    fn version(&self) -> &str {
        "0.1.0"
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
        let probe = PerfEventProbe::new(metric, &events_on_cpus)?;
        alumet.add_source(Box::new(probe));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
