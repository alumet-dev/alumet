use std::time::Duration;

use alumet::pipeline::{Source, Transform, Output};
use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::tokio::{MeasurementPipeline, TaggedSource, SourceType};
use alumet::plugin::{AlumetStart, Plugin};

mod test_plugin;

const VERSION: &str = "0.1.0";

fn main() {
    println!("Starting ALUMET agent v{VERSION}");

    // create the plugins
    println!("Initializing plugins...");
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

    // start the plugins
    let mut metrics = MetricRegistry::new();
    let mut elements = ElementRegistry::new();
    let mut entrypoint = AlumetStart {
        metrics: &mut metrics,
        pipeline_elements: &mut elements,
    };
    for p in plugins.iter_mut() {
        p.start(&mut entrypoint)
            .unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    print_stats(&metrics, &elements, &plugins);

    // start the pipeline
    println!("Starting the pipeline...");
    let tagged = tag_sources(elements.sources);
    let pipeline = MeasurementPipeline::new(tagged, elements.transforms, elements.outputs);
    let _pipeline = pipeline.start(metrics);

    println!("🔥 ALUMET agent is ready");

    // drop the pipeline, wait for the tokio runtime(s) to finish
}

fn print_stats(metrics: &MetricRegistry, elems: &ElementRegistry, plugins: &[Box<dyn Plugin>]) {
    println!("🧩 {} plugins started:", plugins.len());
    for p in plugins {
        println!("  - {} v{}", p.name(), p.version());
    }
    println!(" {} metrics registered: ", metrics.len());
    for m in metrics {
        println!("  - {}: {} ({})", m.name, m.value_type, m.unit);
    }
    println!(
        "📥 {} sources, 🔀 {} transforms and 📝 {} outputs registered.",
        elems.sources.len(),
        elems.transforms.len(),
        elems.outputs.len()
    );
}

fn tag_sources(src: Vec<Box<dyn Source>>) -> Vec<TaggedSource> {
    src.into_iter()
        .map(|src| {
            TaggedSource {
                source: src,
                source_type: SourceType::Normal, // todo get from config
                poll_interval: Duration::from_secs(1),                    // todo get from config
            }
        })
        .collect()
}
