mod common;

use alumet::{
    pipeline::builder::PipelineBuilder,
    plugin::{AlumetStart, Plugin},
};
use common::test_plugin::TestPlugin;

use crate::common::test_plugin::State;

#[test]
fn test_plugin_lifecycle() {
    // Create two TestPlugins with a different name.
    // Each plugin will register 2 metrics, 1 source, 1 transform, 1 output.
    let mut plugins: Vec<Box<TestPlugin>> = vec![TestPlugin::init("plugin1", 98), TestPlugin::init("plugin2", 1000)];

    let mut pipeline_builder = PipelineBuilder::new();

    // Check that creating the PluginStarter does not actually start the plugins.
    assert!(plugins.iter().all(|p| p.state == State::Initialized));

    // Start the plugins
    for p in plugins.iter_mut() {
        let mut handle = AlumetStart::new(&mut pipeline_builder, p.name().to_owned());
        p.start(&mut handle)
            .unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::Started));

    // Check the registration of metrics and pipeline elements
    assert_eq!(4, pipeline_builder.metric_count());
    assert_eq!(2, pipeline_builder.source_count());
    assert_eq!(2, pipeline_builder.transform_count());
    assert_eq!(2, pipeline_builder.output_count());

    let expected_metrics = vec![
        "plugin1:energy-a",
        "plugin1:counter-b",
        "plugin2:energy-a",
        "plugin2:counter-b",
    ];
    assert_eq!(
        sorted(expected_metrics),
        sorted(pipeline_builder.metric_iter().map(|(_id, m)| &m.name).collect())
    );

    // Execute pre-pipeline-start actions.
    let pipeline = pipeline_builder.build().expect("pipeline should build");
    for p in plugins.iter_mut() {
        p.pre_pipeline_start(&pipeline).unwrap_or_else(|err| {
            panic!(
                "Plugin pre_pipeline_start failed: {} v {} - {}",
                p.name(),
                p.version(),
                err
            )
        });
    }
    assert!(plugins.iter().all(|p| p.state == State::PrePipelineStart));

    // Execute post-pipeline-start actions
    let mut pipeline = pipeline.start();
    for p in plugins.iter_mut() {
        p.post_pipeline_start(&mut pipeline).unwrap_or_else(|err| {
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
