use std::time::Duration;
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        Source,
        elements::{
            error::PollError,
            source::trigger,
        },
    },
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::AlumetPlugin,
    },
    resources::{Resource, ResourceConsumer},
    units::Unit,
};

pub struct EnergyToCarbonPlugin;

impl AlumetPlugin for EnergyToCarbonPlugin {
    fn name() -> &'static str {
        "energy-to-carbon" // the name of your plugin, in lowercase, without the "plugin-" prefix
    }

    fn version() -> &'static str {
        log::info!("Version here!!!");
        env!("CARGO_PKG_VERSION") // gets the version from the Cargo.toml of the plugin crate
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        log::info!("Default here!!!");
        Ok(None) // no config for the moment
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        log::info!("Init here!!!");
        Ok(Box::new(EnergyToCarbonPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Start here!!");

        // create a metric for the source
        let energy = alumet.create_metric::<u64> (
            "exemple_energy",
            Unit::Joule,
            "42j sent every seconds (for testing only)",
        )?;

        let not_energy = alumet.create_metric::<u64> (
            "exemple_not_energy",
            Unit::Second,
            "2s sent every seconds (for testing only)",
        )?;


        // create the sources
        let source_energy = ExampleSource {
            metric_energy: energy,
            metric_not_energy: not_energy,
        };

        // How the source is triggered
        let trigger_s = trigger::builder::time_interval(Duration::from_secs(1)).build()?;

        // Add the source to the measurement pipeline
        alumet.add_source("counter", Box::new(source_energy), trigger_s);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Bye!!");
        Ok(())
    }
}

struct ExampleSource {
    metric_energy: TypedMetricId<u64>,
    metric_not_energy: TypedMetricId<u64>,
}
// For testing only
impl Source for ExampleSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Poll !!");

        let point_energy = MeasurementPoint::new(
            timestamp,
            self.metric_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            42,  // Measured value
        );

        let point_not_energy = MeasurementPoint::new(
            timestamp,
            self.metric_not_energy,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            2,  // Measured value
        );
        acc.push(point_energy);
        acc.push(point_not_energy);
        Ok(())
    }
}