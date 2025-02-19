#[cfg(test)]
mod tests {
    use alumet::{
        agent,
        agent::plugin::{PluginInfo, PluginSet},
        plugin::PluginMetadata,
        test::StartupExpectations,
        units::{PrefixedUnit, Unit},
    };
    use plugin_amdgpu::{AmdGpuPlugin, Config};
    use std::time::Duration;

    const TIMEOUT: Duration = Duration::from_secs(2);
    const PLUGIN_NAME: &str = "amdgpu";
    const PLUGIN_SOURCE: &str = "amdgpu";

    fn config_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }

    // define the checks that you want to apply
    #[test]
    fn test() {
        let startup = StartupExpectations::new()
            .expect_metric::<u64>("amd_gpu_clock_frequency", PrefixedUnit::mega(Unit::Hertz))
            .expect_source(PLUGIN_NAME, PLUGIN_SOURCE);

        // start an Alumet agent
        let mut plugins = PluginSet::new();
        let source_config = Config { poll_interval: TIMEOUT };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_table(&source_config)),
        });

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup) // load the checks
            .build_and_start()
            .unwrap();

        // stop the agent
        agent.pipeline.control_handle().shutdown();
        // wait for the agent to stop
        agent.wait_for_shutdown(TIMEOUT).unwrap();
    }
}
