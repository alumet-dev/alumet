use std::time::SystemTime;

use alumet_api::{
    metric::MeasurementPoint,
    plugin::{AlumetStart, Plugin, PluginError, Source},
    units::Unit::Joule,
};

struct RaplPlugin {}

impl Plugin for RaplPlugin {
    fn name(&self) -> &str {
        "rapl"
    }

    fn version(&self) -> &str {
        "0.0.1"
    }

    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), PluginError> {
        // todo provide a way for plugins to emit some data* on start?
        // *that will not change later, e.g. configuration data, list of available domains, etc.
        let domains: Vec<&str> = vec![]; // todo list the domains
        let rapl_metric = alumet
            .metrics
            .new_builder("rapl_energy")
            .description("RAPL energy counter")
            .unit(Joule)
            .build()
            .unwrap();
        for d in domains {}
        Ok(())
    }

    fn stop(&mut self) -> Result<(), PluginError> {
        todo!()
    }
}
