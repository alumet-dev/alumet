mod common;

use alumet::plugin::Plugin;
use alumet::plugin::manage::PluginStartup;
use common::test_plugin::TestPlugin;

use crate::common::test_plugin::State;

#[test]
fn test_plugin_lifecycle() {
    // Create two TestPlugins with a different name.
    // Each plugin will register 2 metrics, 1 source, 1 transform, 1 output.
    let mut plugins: Vec<Box<TestPlugin>> = vec![
        TestPlugin::init("plugin1", 98),
        TestPlugin::init("plugin2", 1000),
    ];
    
    let mut startup = PluginStartup::new();
    
    // Check that creating the PluginStarter does not actually start the plugins.
    assert!(plugins.iter().all(|p| p.state == State::Initialized));

    // Start the plugins
    for p in plugins.iter_mut() {
        startup.start(p.as_mut()).unwrap_or_else(|err| panic!("Plugin failed to start: {} v{} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::Started));

    // Check the registration of metrics and pipeline elements
    assert_eq!(4, startup.metrics.len());
    assert_eq!(2, startup.pipeline_elements.source_count());
    assert_eq!(2, startup.pipeline_elements.transform_count());
    assert_eq!(2, startup.pipeline_elements.output_count());

    let expected_metrics = vec!["plugin1:energy-a", "plugin1:counter-b", "plugin2:energy-a", "plugin2:counter-b"];
    assert_eq!(sorted(expected_metrics), sorted(startup.metrics.iter().map(|m| &m.name).collect()));
    
    // Execute post-startup actions.
    for p in plugins.iter_mut() {
        p.post_startup(&startup).unwrap_or_else(|err| panic!("Plugin post-startup action failed: {} v {} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::PostStartup));

    // Stop the plugins
    for p in plugins.iter_mut() {
        p.stop().unwrap_or_else(|err| panic!("Plugin failed to stop: {} v{} - {}", p.name(), p.version(), err));
    }
    assert!(plugins.iter().all(|p| p.state == State::Stopped));
}

/// Sorts a vector of strings and returns it.
fn sorted<A: AsRef<str> + Ord>(mut strings: Vec<A>) -> Vec<A> {
    strings.sort();
    strings
}
