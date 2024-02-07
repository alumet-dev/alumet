use std::collections::HashMap;
use std::time::{Duration, Instant};

use alumet::pipeline::registry::{ElementRegistry, MetricRegistry};
use alumet::pipeline::tokio::{PendingPipeline, SourceTriggerProvider, SourceType, TaggedOutput, TaggedSource, TaggedTransform, TransformCmd};
use alumet::pipeline::{Output, Source, Transform};
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
    let tagged_sources = tag_sources(elements.sources_per_plugin);
    let tagged_transforms = tag_transforms(elements.transforms_per_plugin);
    let tagged_outputs = tag_outputs(elements.outputs_per_plugin);
    let mut pipeline = PendingPipeline::new(tagged_sources, tagged_transforms, tagged_outputs).start(metrics);
    println!("🔥 ALUMET agent is ready");

    // test commands
    std::thread::sleep(Duration::from_secs(2));
    pipeline.command_all_outputs(alumet::pipeline::tokio::OutputCmd::Pause);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(alumet::pipeline::tokio::SourceCmd::Pause);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_plugin_transforms("test-plugin", TransformCmd::Disable);
    pipeline.command_all_outputs(alumet::pipeline::tokio::OutputCmd::Run);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(alumet::pipeline::tokio::SourceCmd::Run);
    std::thread::sleep(Duration::from_secs(1));
    pipeline.command_all_sources(alumet::pipeline::tokio::SourceCmd::SetTrigger(
        Some(SourceTriggerProvider::TimeInterval { start_time: Instant::now(), poll_interval: Duration::from_millis(100), flush_interval: Duration::from_secs(1) }))
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
        mapvec_count(&elems.sources_per_plugin),
        elems.transforms_per_plugin.len(),
        elems.outputs_per_plugin.len()
    );
}

fn mapvec_count<K, V>(map: &HashMap<K, Vec<V>>) -> usize {
    let mut res = 0;
    for v in map.values() {
        res += v.len();
    }
    res
}

fn tag_sources(src: HashMap<String, Vec<Box<dyn Source>>>) -> Vec<TaggedSource> {
    fn tag(plugin_name: &str, src: Box<dyn Source>) -> TaggedSource {
        let trigger_provider = SourceTriggerProvider::TimeInterval {
            start_time: Instant::now(),
            poll_interval: Duration::from_secs(1),
            flush_interval: Duration::from_secs(1),
        };
        TaggedSource::new(src, SourceType::Normal, trigger_provider, plugin_name.to_owned())
    }

    let mut res = Vec::new();
    for (plugin_name, sources) in src {
        res.extend(sources.into_iter().map(|src| tag(&plugin_name, src)));
    }
    res
}

fn tag_outputs(map: HashMap<String, Vec<Box<dyn Output>>>) -> Vec<TaggedOutput> {
    let mut res = Vec::new();
    for (plugin_name, vec) in map {
        res.extend(vec.into_iter().map(|out| TaggedOutput::new(plugin_name.to_owned(), out)));
    }
    res
}


fn tag_transforms(map: HashMap<String, Vec<Box<dyn Transform>>>) -> Vec<TaggedTransform> {
    let mut res = Vec::new();
    for (plugin_name, vec) in map {
        res.extend(vec.into_iter().map(|tr| TaggedTransform::new(plugin_name.to_owned(), tr)));
    }
    res
}
