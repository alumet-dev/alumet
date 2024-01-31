use std::time::Duration;

use alumet::{pipeline::Source, plugin::Plugin};
use alumet::pipeline::{tokio::MeasurementPipeline, tokio::TaggedSource, registry::Registry};

mod test_plugin;

const VERSION: &str = "0.1.0";

fn main() {
    println!("Starting ALUMET agent v{VERSION}");

    // create the plugins
    println!("Initializing plugins...");
    let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();
    
    // start the plugins
    let mut registries = Registry::new();
    let mut alumet_start = registries.as_start_arg();
    for p in plugins.iter_mut() {
        p.start(&mut alumet_start).unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    print_stats(&registries, &plugins);

    // start the pipeline
    println!("Starting the pipeline...");
    let tagged = tag_sources(registries.sources);
    let pipeline = MeasurementPipeline::new(tagged, registries.transforms, registries.outputs);
    let _pipeline = pipeline.start();

    println!("🔥 ALUMET agent is ready");
    
    // drop the pipeline, wait for the tokio runtime(s) to finish
}

fn print_stats(reg: &Registry, plugins: &[Box<dyn Plugin>]) {
    println!("🧩 {} plugins started:", plugins.len());
    for p in plugins {
        println!("  - {} v{}", p.name(), p.version());
    }
    println!("📥 {} sources, 🔀 {} transforms and 📝 {} outputs registered.", reg.sources.len(), reg.transforms.len(), reg.outputs.len());
}

fn tag_sources(src: Vec<Box<dyn Source>>) -> Vec<TaggedSource> {
    src.into_iter().map(|src| {
        TaggedSource {
            source: src,
            source_type: alumet::pipeline::tokio::SourceType::Normal, // todo get from config
            poll_interval: Duration::from_secs(1), // todo get from config
        }
    }).collect()
}
