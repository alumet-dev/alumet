use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError, elements::source::trigger::TriggerSpec},
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
        // Register the "dummy" metric
        let dummy_metric = alumet
            .create_metric::<u64>(
                "dummy",
                Unit::Custom {
                    unique_name: "g_CO2".to_string(),
                    display_name: "gCO₂".to_string(),
                },
                "Some dummy metric",
            )
            .context("unable to create metric dummy")?;

        // Register the "other" metric used in the grouping test
        let other_metric = alumet
            .create_metric::<u64>("other", Unit::Second, "Another metric for testing grouping")
            .context("unable to create metric other")?;

        alumet.add_source(
            "tests",
            Box::new(TestSource {
                dummy: dummy_metric,
                other: other_metric,
            }),
            TriggerSpec::at_interval(Duration::from_secs(1)),
        )?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct TestSource {
    #[allow(dead_code)]
    dummy: TypedMetricId<u64>,
    #[allow(dead_code)]
    other: TypedMetricId<u64>,
}

impl Source for TestSource {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        // We leave this empty because the test provides the
        // MeasurementBuffer manually via OutputCheckInputContext
        Ok(())
    }
}
