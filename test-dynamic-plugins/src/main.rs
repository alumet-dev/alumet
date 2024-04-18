use std::env;
use std::path::Path;
use std::time::Duration;

use alumet::{pipeline::builder::PipelineBuilder, plugin::{dynload::{initialize, load_cdylib, plugin_subconfig}, AlumetStart}};

fn main() {
    // read arguments
    let args: Vec<String> = env::args().collect();
    let plugin_file = &args[1];
    let expected_plugin_name = &args[2];
    let expected_plugin_version = &args[3];

    // create ad-hoc global config for testing
    let mut global_config = toml::Table::new();
    let mut plugin_config = toml::Table::new();
    plugin_config.insert("custom_attribute".into(), "42".into());
    global_config.insert(expected_plugin_name.into(), plugin_config.into());

    // run the test
    let plugin_file = Path::new(plugin_file);
    run_with_plugin(
        plugin_file,
        expected_plugin_name,
        expected_plugin_version,
        &mut global_config,
        Duration::from_secs(2),
    );
}

fn run_with_plugin(
    plugin_file: &Path,
    expected_plugin_name: &str,
    expected_plugin_version: &str,
    global_config: &mut toml::Table,
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

    // Create the plugin
    let plugin_config = plugin_subconfig(&plugin_info, global_config).expect("plugin subconfig should exist");
    println!("[app] plugin_config: {:?}", plugin_config.0);
    let mut plugin = initialize(plugin_info, plugin_config).expect("plugin instance should be created by init");
    assert_eq!(plugin.name(), expected_plugin_name);
    assert_eq!(plugin.version(), expected_plugin_version);

    // Start the plugin
    let mut pipeline_builder = PipelineBuilder::new();
    let mut handle = AlumetStart::new(&mut pipeline_builder, plugin.name().to_owned());
    plugin.start(&mut handle).expect("plugin should start fine");

    // start the pipeline and wait for the tasks to finish
    println!("[app] Starting the pipeline...");
    let pipeline = pipeline_builder.build().expect("pipeline should build").start();

    println!("[app] pipeline started");

    // keep the pipeline running for some time
    std::thread::sleep(duration);
    drop(pipeline);
}
