use std::time::{Duration, Instant};

use alumet::pipeline;
use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::runtime::{ConfiguredSource, MeasurementPipeline, SourceType};
use alumet::pipeline::trigger::TriggerProvider;
use alumet::plugin::{Plugin, PluginStarter};

use env_logger::Env;

use crate::socket_control::SocketControl;

mod default_plugin;
mod socket_control;
mod output_csv;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    log::info!("Starting ALUMET agent v{VERSION}");

    // create the plugins
    log::info!("Starting plugins...");
    let mut plugins: Vec<Box<dyn Plugin>> = vec![
        Box::new(default_plugin::DefaultPlugin),
        Box::new(plugin_rapl::RaplPlugin),
    ];

    // start the plugins
    let mut metrics = MetricRegistry::new();
    let mut elements = ElementRegistry::new();
    let mut starter = PluginStarter::new(&mut metrics, &mut elements);
    for p in plugins.iter_mut() {
        starter
            .start(p.as_mut())
            .unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    print_stats(&metrics, &elements, &plugins);

    // start the pipeline and wait for the tasks to finish
    log::info!("Starting the pipeline...");
    let mut pipeline = MeasurementPipeline::with_settings(elements, apply_source_settings).start(metrics);
    
    log::info!("Starting socket control...");
    let control = SocketControl::start_new(pipeline.control_handle()).expect("Control thread failed to start");
    
    log::info!("🔥 ALUMET agent is ready");

    // keep the pipeline running until the app closes
    pipeline.wait_for_all();
    control.stop();
    control.join();
}

fn print_stats(metrics: &MetricRegistry, elems: &ElementRegistry, plugins: &[Box<dyn Plugin>]) {
    // plugins
    let plugins_list = plugins
        .iter()
        .map(|p| format!("    - {} v{}", p.name(), p.version()))
        .collect::<Vec<_>>()
        .join("\n");

    let metrics_list = metrics
        .iter()
        .map(|m| format!("    - {}: {} ({})", m.name, m.value_type, m.unit))
        .collect::<Vec<_>>()
        .join("\n");

    let pipeline_elements = format!(
        "📥 {} sources, 🔀 {} transforms and 📝 {} outputs registered.",
        elems.source_count(),
        elems.transform_count(),
        elems.output_count()
    );

    let n_plugins = plugins.len();
    let n_metrics = metrics.len();
    let str_plugin = if n_plugins > 1 { "plugins" } else { "plugin" };
    let str_metric = if n_metrics > 1 { "metrics" } else { "metric" };
    log::info!("Plugin startup complete.\n🧩 {n_plugins} {str_plugin} started:\n{plugins_list}.\n📏 {n_metrics} {str_metric} registered:\n{metrics_list}\n{pipeline_elements}");
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
