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
        env!("CARGO_PKG_VERSION") // gets the version from the Cargo.toml of the plugin crate
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None) // no config for the moment
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(EnergyToCarbonPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Hello!");

        // create a metric for the source
        let counter_metric = alumet.create_metric::<u64> (
            "exemple_source_call_counter",
            Unit::Unity,
            "number of times the example source has been called",
        )?;

        // create the sources
        let source = ExampleSource {
            metric: counter_metric,
            counter: 0,
        };

        // How the source is triggered
        let trigger = trigger::builder::time_interval(Duration::from_secs(1)).build()?;

        // Add the source to the measurement pipeline
        alumet.add_source("counter", Box::new(source), trigger);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Bye!");
        Ok(())
    }
}

struct ExampleSource {
    metric: TypedMetricId<u64>,
    counter: u64,
}

impl Source for ExampleSource {
    fn poll(&mut self, acc: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let n_calls = self.counter;
        self.counter += 1;

        let point = MeasurementPoint::new(
            timestamp,
            self.metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            n_calls,  // Measured value
        );
        acc.push(point);
        Ok(())
    }
}