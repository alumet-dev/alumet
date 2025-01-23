use alumet::{agent, pipeline, plugin::PluginMetadata};

use anyhow::Context;
use std::{thread, time, time::Duration};

use super::points::{catch_error_point, catch_panic_point};

pub(super) fn build_and_run(mut plugins: Vec<PluginMetadata>) -> anyhow::Result<()> {
    // Generate the default configuration
    let mut config = catch_error_point!(agent_default_config, || {
        let mut config = toml::Table::new();
        agent::config::insert_default_plugin_configs(&plugins, &mut config)?;
        Ok(config)
    });

    // Build an agent with the plugins and their configs.
    let agent = catch_error_point!(agent_build_and_start, move || {
        let mut pipeline_builder = pipeline::Builder::new();
        pipeline_builder.trigger_constraints_mut().max_update_interval = Duration::from_millis(100);

        let mut agent_builder = agent::Builder::new(pipeline_builder);

        let config_per_plugin = agent::config::extract_plugin_configs(&mut config).expect("config should be valid");
        for (plugin_name, (enabled, config)) in config_per_plugin {
            let i = plugins
                .iter()
                .position(|p| p.name == plugin_name)
                .expect("plugin should exist");
            let plugin = plugins.swap_remove(i);
            agent_builder.add_plugin(plugin, enabled, config);
        }

        agent_builder.build_and_start()
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
