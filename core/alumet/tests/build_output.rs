use std::{
    thread,
    time::{self, Duration},
};

use alumet::{
    agent::{self, plugin::PluginSet},
    pipeline::{self, elements::output::AsyncOutputStream},
    plugin::{AlumetPluginStart, ConfigTable, rust::AlumetPlugin},
    static_plugins,
};
use anyhow::Context;

struct TestPlugin;

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "build_output"
    }

    fn version() -> &'static str {
        "0.0.1"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(TestPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        alumet.add_async_output_builder("out", |_ctx, input| {
            tokio::runtime::Handle::try_current()
                .expect("the builder should be called in the context of a tokio runtime");
            let output = async_output_run(input);
            Ok(Box::pin(output))
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn async_output_run(_input: AsyncOutputStream) -> anyhow::Result<()> {
    Ok(())
}

#[test]
fn ensure_async_build_output_in_context() -> anyhow::Result<()> {
    env_logger::init();

    // Create an agent with the plugin
    let plugins = static_plugins![TestPlugin];
    let plugins = PluginSet::from(plugins);

    let mut pipeline_builder = pipeline::Builder::new();
    pipeline_builder.trigger_constraints_mut().max_update_interval = Duration::from_millis(100);

    let agent_builder = agent::Builder::from_pipeline(plugins, pipeline_builder);

    // Start Alumet
    let agent = agent_builder.build_and_start().expect("agent should start fine");

    // Stop Alumet
    thread::sleep(time::Duration::from_millis(100));
    agent.pipeline.control_handle().shutdown();

    // Ensure that Alumet has stopped quickly
    let timeout_duration = Duration::from_secs(1);
    agent
        .wait_for_shutdown(timeout_duration)
        .context("error while shutting down")?;
    Ok(())
}
