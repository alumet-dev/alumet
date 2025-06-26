use std::time::Duration;

use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        elements::{error::PollError, source::trigger},
        Source,
    },
    plugin::{rust::AlumetPlugin, AlumetPluginStart, ConfigTable},
    resources::{Resource, ResourceConsumer},
    units::Unit,
};

pub struct TestsPlugin;

impl AlumetPlugin for TestsPlugin {
    fn name() -> &'static str {
        "fackplugin"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(TestsPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let counter_metric = alumet.create_metric::<u64>(
            "example_counter",
            Unit::Unity,
            "number of times the example source has been called", // description
        )?;

        // Create the source
        let source = ExampleSource {
            metric: counter_metric,
            counter: 0,
        };

        let trigger = trigger::builder::time_interval(Duration::from_secs(1)).build()?;
        let res = alumet.add_source("kwollect_source", Box::new(source), trigger);
        assert!(res.is_ok());

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
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
            n_calls, // measured value
        )
        .with_attr("agence", AttributeValue::String("CHERUB".to_string()))
        .with_attr("Fondation", AttributeValue::U64(1946));

        acc.push(point.clone());
        if self.counter % 2 == 0 {
            acc.push(point);
        }
        Ok(())
    }
}
