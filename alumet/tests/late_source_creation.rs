use std::{
    thread,
    time::{self, Duration},
};

use alumet::{
    agent::{AgentBuilder, AgentConfig},
    measurement::{MeasurementAccumulator, Timestamp},
    pipeline::{elements::error::PollError, trigger::TriggerSpec},
    plugin::{
        rust::{serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
    static_plugins,
};
use anyhow::Context;

struct TestPlugin;

struct TestSource;

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "late_source_creation"
    }

    fn version() -> &'static str {
        "0.0.1"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Duration::from_secs(1))?;
        Ok(Some(config))
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(TestPlugin))
    }

    fn start(&mut self, _alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // No source creation here
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        control_handle
            .add_source(
                "x",
                Box::new(TestSource),
                TriggerSpec::at_interval(Duration::from_secs(1)),
            )
            .context("failed to add source in post_pipeline_start")?;
        Ok(())
    }
}

impl alumet::pipeline::Source for TestSource {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, _t: Timestamp) -> Result<(), PollError> {
        Ok(())
    }
}

#[test]
fn late_source_creation_test() -> anyhow::Result<()> {
    env_logger::init();

    // Create an agent with the plugin
    let plugins = static_plugins![TestPlugin];
    let mut agent = AgentBuilder::new(plugins).config_value(toml::Table::new()).build();

    // Start Alumet
    let global_config = agent.default_config()?;
    let agent_config = AgentConfig::try_from(global_config)?;
    agent.source_trigger_constraints().max_update_interval = Duration::from_millis(100);
    let agent = agent.start(agent_config)?;

    // Wait for the source to be registered
    thread::sleep(time::Duration::from_secs(1));

    // Stop Alumet
    agent.pipeline.control_handle().shutdown();

    // Ensure that Alumet has stopped in less than x seconds
    let timeout_duration = Duration::from_secs_f32(1.5);
    agent
        .wait_for_shutdown(timeout_duration)
        .context("error while shutting down")?;
    Ok(())
}
