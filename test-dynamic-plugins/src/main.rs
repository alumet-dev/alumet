use std::env;
use std::path::Path;
use std::time::Duration;

use alumet::{
    agent::{AgentBuilder, AgentConfig},
    plugin::dynload::load_cdylib,
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

    // create ad-hoc global config for testing
    let mut global_config = toml::Table::new();
    let mut plugins_section = toml::Table::new();
    let mut plugin_config = toml::Table::new();
    plugin_config.insert("custom_attribute".into(), "42".into());
    plugins_section.insert(expected_plugin_name.into(), plugin_config.into());
    global_config.insert("plugins".into(), plugins_section.into());

    // run the test
    let plugin_file = Path::new(plugin_file);
    run_with_plugin(
        plugin_file,
        expected_plugin_name,
        expected_plugin_version,
        global_config,
        Duration::from_secs(2),
    );
}

fn run_with_plugin(
    plugin_file: &Path,
    expected_plugin_name: &str,
    expected_plugin_version: &str,
    global_config: toml::Table,
    duration: Duration,
) {
    println!("[app] Starting ALUMET");

    // Load the dynamic plugin
    let plugin_info = load_cdylib(plugin_file).expect("failed to load plugin");
    println!(
        "[app] dynamic plugin loaded: {} version {}",
        plugin_info.name, plugin_info.version
    );
    assert_eq!(plugin_info.name, expected_plugin_name);
    assert_eq!(plugin_info.version, expected_plugin_version);

    // Check the config (print it here and assert_eq on the output in tests/test_plugins.rs)
    println!("[app] global config: {:?}", global_config);

    // Create an agent with the plugin
    let expected_plugin_name = expected_plugin_name.to_owned();
    let expected_plugin_version = expected_plugin_version.to_owned();
    let agent = AgentBuilder::new(vec![plugin_info])
        .after_plugin_init(move |plugins| {
            let plugin = &plugins[0];
            assert_eq!(plugin.name(), expected_plugin_name);
            assert_eq!(plugin.version(), expected_plugin_version);
        })
        .after_plugin_start(|_| println!("[app] plugin started"))
        .before_operation_begin(|_| {
            println!("[app] Starting the pipeline...");
        })
        .after_operation_begin(|_| println!("[app] pipeline started"))
        .build();

    // Start the plugin and the pipeline
    let agent_config = AgentConfig::try_from(global_config).expect("config should be valid");
    let running = agent.start(agent_config).expect("agent should start fine");

    // keep the pipeline running for some time
    std::thread::sleep(duration);
    println!("[app] shutting down...");
    running.pipeline.control_handle().shutdown();
    running.wait_for_shutdown().expect("error in shutdown");
    println!("[app] stop");
}
