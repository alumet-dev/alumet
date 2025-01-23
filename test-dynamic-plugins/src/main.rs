use std::env;
use std::path::Path;
use std::time::Duration;

use alumet::{
    agent,
    plugin::{dynload::load_cdylib, PluginMetadata},
};

fn main() {
    // init logger
    env_logger::builder()
        .is_test(true)
        .target(env_logger::Target::Stdout)
        .format_timestamp(None) // no timestamp (because we compare the output in the test)
        .filter_level(log::LevelFilter::Warn) // only show warnings and errors
        .init();

    // read arguments
    let args: Vec<String> = env::args().collect();
    let plugin_file = &args[1];
    let expected_plugin_name = &args[2];
    let expected_plugin_version = &args[3];

    // create the plugin config
    let mut plugin_config = toml::Table::new();
    plugin_config.insert("custom_attribute".into(), "42".into());

    // run the test
    let plugin_file = Path::new(plugin_file);
    run_with_plugin(
        plugin_file,
        expected_plugin_name,
        expected_plugin_version,
        plugin_config,
        Duration::from_secs(2),
    );
}

fn run_with_plugin(
    plugin_file: &Path,
    expected_plugin_name: &str,
    expected_plugin_version: &str,
    plugin_config: toml::Table,
    duration: Duration,
) {
    println!("[app] Starting ALUMET");

    // Load the dynamic plugin
    let plugin: PluginMetadata = load_cdylib(plugin_file).expect("failed to load plugin");
    println!(
        "[app] dynamic plugin loaded: {} version {}",
        plugin.name, plugin.version
    );
    assert_eq!(plugin.name, expected_plugin_name);
    assert_eq!(plugin.version, expected_plugin_version);

    // Check the config (print it here and assert_eq on the output in tests/test_plugins.rs)
    println!("[app] plugin config: {:?}", plugin_config);

    // Prepare for checks
    let expected_plugin_name = expected_plugin_name.to_owned();
    let expected_plugin_version = expected_plugin_version.to_owned();

    // Disable high-priority threads, they are useless for the dynamic plugin tests
    // and don't work in CI.
    let mut pipeline_builder = alumet::pipeline::Builder::new();
    pipeline_builder.high_priority_threads(0);

    // Build and agent with the plugin
    let mut agent_builder = agent::Builder::new(pipeline_builder);
    agent_builder.add_plugin(plugin, true, plugin_config);
    agent_builder
        .after_plugins_init(move |plugins| {
            let plugin = &plugins[0];
            assert_eq!(plugin.name(), expected_plugin_name);
            assert_eq!(plugin.version(), expected_plugin_version);
        })
        .after_plugins_start(|_| println!("[app] plugin started"))
        .before_operation_begin(|_| {
            println!("[app] Starting the pipeline...");
        })
        .after_operation_begin(|_| println!("[app] pipeline started"));

    // Start the plugin and the pipeline
    let agent = agent_builder.build_and_start().expect("agent should start fine");
    const TIMEOUT: Duration = Duration::from_secs(2);

    // keep the pipeline running for some time
    std::thread::sleep(duration);
    println!("[app] shutting down...");
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).expect("error in shutdown");
    println!("[app] stop");
}
