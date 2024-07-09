mod common;

use alumet::{
    pipeline::{self, PluginName},
    plugin::{AlumetPostStart, AlumetStart, Plugin},
};
use common::test_plugin::TestPlugin;

use crate::common::test_plugin::State;

#[test]
fn test_plugin_lifecycle() {
    // Create two TestPlugins with a different name.
    // Each plugin will register 2 metrics, 1 source, 1 transform, 1 output.
    let mut plugins: Vec<Box<TestPlugin>> = vec![TestPlugin::init("plugin1", 98), TestPlugin::init("plugin2", 1000)];

    let mut pipeline_builder = pipeline::Builder::new();

    // Check that creating the PluginStarter does not actually start the plugins.
    assert!(plugins.iter().all(|p| p.state == State::Initialized));

    // Start the plugins
    for p in plugins.iter_mut() {
        let mut handle = AlumetStart::new(&mut pipeline_builder, PluginName(p.name().to_owned()));
        p.start(&mut handle)
            .unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::Started));

    // Check the registration of metrics and pipeline elements
    let stats = pipeline_builder.stats();
    assert_eq!(4, stats.metrics);
    assert_eq!(2, stats.sources);
    assert_eq!(2, stats.transforms);
    assert_eq!(2, stats.outputs);

    let expected_metrics = vec![
        "plugin1:energy-a",
        "plugin1:counter-b",
        "plugin2:energy-a",
        "plugin2:counter-b",
    ];
    assert_eq!(
        sorted(expected_metrics),
        pipeline_builder.peek_metrics(|m| m.iter().map(|(_id, m)| m.name.clone()).collect::<Vec<_>>())
    );

    // Build and start the pipeline.
    let mut pipeline = pipeline_builder.build().expect("pipeline should build");

    // Execute post-pipeline-start actions
    for p in plugins.iter_mut() {
        let mut alumet = AlumetPostStart::new(&mut pipeline, PluginName(p.name().to_owned()));
        p.post_pipeline_start(&mut alumet).unwrap_or_else(|err| {
            panic!(
                "Plugin post_pipeline_start failed: {} v {} - {}",
                p.name(),
                p.version(),
                err
            )
        });
    }
    assert!(plugins.iter().all(|p| p.state == State::PostPipelineStart));

    // Stop the plugins
    for p in plugins.iter_mut() {
        p.stop()
            .unwrap_or_else(|err| panic!("Plugin failed to stop: {} v{} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::Stopped));

    // Stop the pipeline (because it is dropped here).
}

/// Sorts a vector of strings and returns it.
fn sorted<A: AsRef<str> + Ord>(mut strings: Vec<A>) -> Vec<A> {
    strings.sort();
    strings
}
