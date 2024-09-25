use alumet::agent::{AgentBuilder, AgentConfig};
use alumet::plugin::PluginMetadata;

use anyhow::Context;
use std::{thread, time, time::Duration};

use super::points::{catch_error_point, catch_panic_point};

pub(super) fn build_and_run(plugins: Vec<PluginMetadata>) -> anyhow::Result<()> {
    // Create an agent with the plugin
    let mut agent = catch_panic_point!(agent_build, move || {
        AgentBuilder::new(plugins).config_value(toml::Table::new()).build()
    });

    // Start Alumet
    let global_config = catch_error_point!(agent_default_config, || { agent.default_config() });

    let agent_config = catch_error_point!(agent_config_from, move || { AgentConfig::try_from(global_config) });
    agent.source_trigger_constraints().max_update_interval = Duration::from_millis(100);

    let agent = catch_error_point!(agent_start, || { agent.start(agent_config) });

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
