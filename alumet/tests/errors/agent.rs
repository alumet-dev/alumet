use alumet::{
    agent::{
        self,
        config::{AutoDefaultConfigProvider, DefaultConfigProvider},
        plugin::PluginSet,
    },
    pipeline,
    plugin::PluginMetadata,
};

use anyhow::Context;
use std::{thread, time, time::Duration};

use super::points::{catch_error_point, catch_panic_point};

pub(super) fn build_and_run(plugins: Vec<PluginMetadata>) -> anyhow::Result<()> {
    let mut plugins = PluginSet::new(plugins);

    // Generate the default configuration
    let mut config = catch_error_point!(agent_default_config, || {
        let config = AutoDefaultConfigProvider::new(&plugins, || toml::Table::new()).default_config()?;
        Ok(config)
    });

    // Build an agent with the plugins and their configs.
    let agent = catch_error_point!(agent_build_and_start, move || {
        // Apply some setting to the pipeline to shorten the test duration.
        let mut pipeline_builder = pipeline::Builder::new();
        pipeline_builder.trigger_constraints_mut().max_update_interval = Duration::from_millis(100);

        // Extract the plugins configs and enable/disable the plugins according to their config.
        plugins
            .extract_config(&mut config, true, agent::plugin::UnknownPluginInConfigPolicy::Error)
            .expect("config should be valid");

        // Build the agent
        agent::Builder::from_pipeline(plugins, pipeline_builder).build_and_start()
    });

    // Wait for the source to be registered and run a bit
    thread::sleep(time::Duration::from_secs(1));

    // Stop Alumet
    catch_panic_point!(shutdown, || {
        agent.pipeline.control_handle().shutdown();
    });

    // Ensure that Alumet has stopped in less than x seconds
    let timeout_duration = Duration::from_secs_f32(1.5);
    catch_error_point!(wait_for_shutdown, move || {
        agent
            .wait_for_shutdown(timeout_duration)
            .context("error while shutting down")
    });
    Ok(())
}
