use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, elements::source::trigger::TriggerSpec, Source},
    plugin::rust::AlumetPlugin,
    units::Unit,
};
use anyhow::Context;
use std::time::Duration;

pub struct TestsPlugin;

impl AlumetPlugin for TestsPlugin {
    fn name() -> &'static str {
        "tests"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let dumb_metric = alumet
            .create_metric::<u64>("dumb", Unit::Second, "Some dumb metric")
            .context("unable to create metric test")?;
        alumet.add_source(
            "tests",
            Box::new(TestSource { dumb: dumb_metric }),
            TriggerSpec::at_interval(Duration::from_secs(1)),
        )?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct TestSource {
    dumb: TypedMetricId<u64>,
}

impl Source for TestSource {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        Ok(())
    }
}
