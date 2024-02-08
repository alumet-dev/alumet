use std::time::{Duration, Instant};

use alumet::pipeline;
use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::runtime::{MeasurementPipeline, SourceType, ConfiguredSource, TransformCmd, SourceCmd, OutputCmd};
use alumet::pipeline::trigger::TriggerProvider;
use alumet::plugin::{Plugin, PluginStarter};

mod test_plugin;

const VERSION: &str = "0.1.0";

fn main() {
    println!("Starting ALUMET agent v{VERSION}");

    // create the plugins
    println!("Initializing plugins...");
    let mut plugins: Vec<Box<dyn Plugin>> = vec![test_plugin::TestPlugin::init()];

    // start the plugins
    let mut metrics = MetricRegistry::new();
    let mut elements = ElementRegistry::new();
    let mut starter = PluginStarter::new(&mut metrics, &mut elements);
    for p in plugins.iter_mut() {
        starter
            .start(p)
            .unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    print_stats(&metrics, &elements, &plugins);

    // start the pipeline and wait for the tasks to finish
    println!("Starting the pipeline...");
    let mut pipeline = MeasurementPipeline::with_settings(elements, apply_source_settings).start(metrics);
    println!("🔥 ALUMET agent is ready");

    // test commands
    std::thread::sleep(Duration::from_secs(2));
    pipeline.command_all_outputs(OutputCmd::Pause);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(SourceCmd::Pause);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_plugin_transforms("test-plugin", TransformCmd::Disable);
    pipeline.command_all_outputs(OutputCmd::Run);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(SourceCmd::Run);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(SourceCmd::SetTrigger(
        Some(TriggerProvider::TimeInterval { start_time: Instant::now(), poll_interval: Duration::from_millis(100), flush_interval: Duration::from_secs(1) }))
    );
    std::thread::sleep(Duration::from_secs(3));
    pipeline.command_plugin_transforms("test-plugin", TransformCmd::Enable);
    // keep the pipeline running until the app closes
    pipeline.wait_for_all();
}

fn print_stats(metrics: &MetricRegistry, elems: &ElementRegistry, plugins: &[Box<dyn Plugin>]) {
    // plugins
    println!("🧩 {} plugins started:", plugins.len());
    for p in plugins {
        println!("- {} v{}", p.name(), p.version());
    }

    // metrics
    println!("📏 {} metrics registered: ", metrics.len());
    for m in metrics {
        println!("- {}: {} ({})", m.name, m.value_type, m.unit);
    }

    // pipeline elements
    println!(
        "📥 {} sources, 🔀 {} transforms and 📝 {} outputs registered.",
        elems.source_count(),
        elems.transform_count(),
        elems.output_count()
    );
}

fn apply_source_settings(source: Box<dyn pipeline::Source>, plugin_name: String) -> ConfiguredSource {
    // normally this would be fetched from the config
    let source_type = SourceType::Normal;
    let trigger_provider = TriggerProvider::TimeInterval {
        start_time: Instant::now(),
        poll_interval: Duration::from_secs(1),
        flush_interval: Duration::from_secs(1),
    };
    ConfiguredSource { source, plugin_name, source_type, trigger_provider }
}
