use std::collections::HashMap;
use std::time::Duration;

use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::tokio::{MeasurementPipeline, MeasurementPipelineBuilder, SourceType, TaggedSource};
use alumet::pipeline::{Output, Source, Transform};
use alumet::plugin::{AlumetStart, Plugin, PluginStarter};

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
    let tagged = tag_sources(elements.sources_per_plugin);
    let pipeline = MeasurementPipelineBuilder::new(tagged, elements.transforms, elements.outputs)
        .build()
        .expect("Async runtime failed to build.");
    pipeline.run(metrics, || println!("🔥 ALUMET agent is ready"));
}

fn print_stats(metrics: &MetricRegistry, elems: &ElementRegistry, plugins: &[Box<dyn Plugin>]) {
    println!("🧩 {} plugins started:", plugins.len());
    for p in plugins {
        println!("  - {} v{}", p.name(), p.version());
    }
    println!("📏 {} metrics registered: ", metrics.len());
    for m in metrics {
        println!("  - {}: {} ({})", m.name, m.value_type, m.unit);
    }
    println!(
        "📥 {} sources, 🔀 {} transforms and 📝 {} outputs registered.",
        mapvec_count(&elems.sources_per_plugin),
        elems.transforms.len(),
        elems.outputs.len()
    );
}

fn mapvec_count<K, V>(map: &HashMap<K, Vec<V>>) -> usize {
    let mut res = 0;
    for (k, v) in map {
        res += v.len();
    }
    res
}

fn tag_sources(src: HashMap<String, Vec<Box<dyn Source>>>) -> Vec<TaggedSource> {
    let mut res = Vec::new();
    for (plugin_name, sources) in src {
        res.extend(
            sources
                .into_iter()
                .map(|src| TaggedSource::new(src, SourceType::Normal, Duration::from_secs(1), plugin_name.clone())),
        );
    }
    res
}
