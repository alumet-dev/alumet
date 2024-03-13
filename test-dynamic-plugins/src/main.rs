use std::env;
use std::path::Path;
use std::time::{Duration, Instant};

use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::runtime::{ConfiguredSource, MeasurementPipeline, SourceType};
use alumet::pipeline::trigger::TriggerProvider;
use alumet::plugin::dynlib::loading::{initialize, load_cdylib, plugin_subconfig};
use alumet::plugin::PluginStarter;
use alumet::{config, pipeline};

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
    );
}

fn run_with_plugin(
    plugin_file: &Path,
    expected_plugin_name: &str,
    expected_plugin_version: &str,
    global_config: &mut toml::Table,
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
    println!("[app] plugin_config: {plugin_config:?}");
    let mut plugin = initialize(plugin_info, plugin_config).expect("plugin instance should be created by init");
    assert_eq!(plugin.name(), expected_plugin_name);
    assert_eq!(plugin.version(), expected_plugin_version);

    // Start the plugin
    let mut metrics = MetricRegistry::new();
    let mut elements = ElementRegistry::new();
    let mut starter = PluginStarter::new(&mut metrics, &mut elements);
    starter.start(plugin.as_mut()).expect("plugin should start fine");

    // start the pipeline and wait for the tasks to finish
    println!("[app] Starting the pipeline...");
    let mut pipeline = MeasurementPipeline::with_settings(elements, apply_source_settings).start(metrics);

    println!("[app] pipeline started");

    // keep the pipeline running until the app closes
    pipeline.wait_for_all();
}

fn apply_source_settings(source: Box<dyn pipeline::Source>, plugin_name: String) -> ConfiguredSource {
    // normally this would be fetched from the config
    let source_type = SourceType::Normal;
    let trigger_provider = TriggerProvider::TimeInterval {
        start_time: Instant::now(),
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
    };
    ConfiguredSource {
        source,
        plugin_name,
        source_type,
        trigger_provider,
    }
}
