use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    pipeline::{
        Source,
        elements::{error::PollError, source::trigger::TriggerSpec},
    },
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, serialize_config},
    },
    units::Unit,
};
use anyhow::Context;
use plugin_csv::Config;
use std::time::Duration;

pub struct TestsPlugin;

impl AlumetPlugin for TestsPlugin {
    fn name() -> &'static str {
        "test-plugin"
    }

    fn version() -> &'static str {
        "0.2.0"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let dumb_metric = alumet
            .create_metric::<u64>("dumb", Unit::Unity, "Some dumb metric")
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

#[allow(dead_code)]
struct TestSource {
    dumb: TypedMetricId<u64>,
}

impl Source for TestSource {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        pipeline,
        plugin::PluginMetadata,
    };
    use plugin_csv::Config;
    use std::time::Duration;

    use super::TestsPlugin;

    const TIMEOUT: Duration = Duration::from_secs(10);

    // TODO move this duplicated function
    fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    #[test]
    fn test_start_stop() {
        let default_config = Config { ..Default::default() };

        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<TestsPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&default_config)),
        });

        // Set up the measurement pipeline
        let mut pipeline = pipeline::Builder::new();
        pipeline.normal_threads(2); // Example setting: use 2 threads to run async pipeline elements

        // Build and start the agent
        let agent = agent::Builder::from_pipeline(plugins, pipeline)
            .build_and_start()
            .expect("startup failure");

        let handle = agent.pipeline.control_handle();
        //force shutdown (else timeout and test failed)
        handle.shutdown();

        agent.wait_for_shutdown(TIMEOUT).expect("error while running");
    }
}
