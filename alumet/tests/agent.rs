use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use alumet::{
    agent::{
        self,
        config::{AutoDefaultConfigProvider, DefaultConfigProvider},
        plugin::PluginSet,
    },
    plugin::{
        rust::{serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable, PluginMetadata,
    },
    static_plugins,
};
use serde::Serialize;

mod common;
use common::test_plugin::{AtomicState, MeasurementCounters, State, TestPlugin};
use toml::toml;

#[test]
fn static_plugin_macro() {
    let empty = static_plugins![];
    assert!(empty.is_empty());

    let single = static_plugins![MyPlugin];
    assert_eq!(1, single.len());
    assert_eq!("name", single[0].name);
    assert_eq!("version", single[0].version);

    // Accept single identifiers and qualified paths.
    let multiple = static_plugins![MyPlugin, self::MyPlugin];
    assert_eq!(2, multiple.len());
}

#[test]
fn default_config_no_plugin() {
    let plugins = PluginSet::new(Vec::new()); // empty set

    let config = AutoDefaultConfigProvider::new(&plugins, || toml::Table::new())
        .default_config()
        .unwrap();
    let expected = toml::toml! {
        [plugins]
    };
    assert_eq!(config, expected);
}

#[test]
fn default_config_1_plugin() {
    let plugins = PluginSet::new(static_plugins![MyPlugin]);
    let config = AutoDefaultConfigProvider::new(&plugins, MyAgentConfig::default)
        .default_config()
        .unwrap();

    let expected = toml! {
        global_setting = "default"

        [plugins.name]
        list = ["default-item"]
        count = 42

    };
    assert_eq!(config, expected);
}

#[test]
fn test_plugin_lifecycle() {
    // Create two TestPlugins with a different name.
    // Each plugin will register 2 metrics, 1 source, 1 transform, 1 output.
    let state1 = Arc::new(AtomicState::new(State::PreInit));
    let state2 = Arc::new(AtomicState::new(State::PreInit));
    let counters1 = MeasurementCounters::default();
    let counters2 = MeasurementCounters::default();
    const COUNTER_ORD: Ordering = Ordering::Relaxed;

    let (state1_meta, state2_meta) = (state1.clone(), state2.clone());
    let (c1_meta, c2_meta) = (counters1.clone(), counters2.clone());
    let plugins = vec![
        PluginMetadata {
            name: "plugin1".to_owned(),
            version: "0.0.1".to_owned(),
            init: Box::new(move |_| Ok(TestPlugin::init("plugin1", 98, state1_meta, c1_meta))),
            default_config: Box::new(|| Ok(None)),
        },
        PluginMetadata {
            name: "plugin2".to_owned(),
            version: "0.0.1".to_owned(),
            init: Box::new(move |_| Ok(TestPlugin::init("plugin2", 1000, state2_meta, c2_meta))),
            default_config: Box::new(|| Ok(None)),
        },
    ];
    let plugins = PluginSet::new(plugins);

    let (state1_init, state2_init) = (state1.clone(), state2.clone());
    let (state1_start, state2_start) = (state1.clone(), state2.clone());
    let (state1_pre_op, state2_pre_op) = (state1.clone(), state2.clone());
    let (state1_op, state2_op) = (state1.clone(), state2.clone());
    let (counters1_pre_op, counters2_pre_op) = (counters1.clone(), counters2.clone());

    let builder = agent::Builder::new(plugins)
        .after_plugins_init(move |_| {
            // check plugin initialization
            assert_eq!(state1_init.get(), State::Initialized);
            assert_eq!(state2_init.get(), State::Initialized);
        })
        .after_plugins_start(move |builder| {
            // check plugin startup
            assert_eq!(state1_start.get(), State::Started);
            assert_eq!(state2_start.get(), State::Started);

            // check builder statistics
            let stats = builder.stats();
            assert_eq!(4, stats.metrics);
            assert_eq!(2, stats.sources);
            assert_eq!(2, stats.transforms);
            assert_eq!(2, stats.outputs);

            // check metrics
            let expected_metrics = vec![
                "plugin1:energy-a",
                "plugin1:counter-b",
                "plugin2:energy-a",
                "plugin2:counter-b",
            ];
            assert_eq!(
                sorted(expected_metrics),
                sorted(
                    builder
                        .metrics()
                        .iter()
                        .map(|(_id, m)| m.name.clone())
                        .collect::<Vec<_>>()
                )
            );
        })
        .before_operation_begin(move |_| {
            // check pipeline startup
            assert_eq!(state1_pre_op.get(), State::PrePipelineStart);
            assert_eq!(state2_pre_op.get(), State::PrePipelineStart);

            // check no measurements have been produced yet
            assert_eq!(counters1_pre_op.n_polled.load(COUNTER_ORD), 0);
            assert_eq!(counters1_pre_op.n_transform_in.load(COUNTER_ORD), 0);
            assert_eq!(counters1_pre_op.n_transform_out.load(COUNTER_ORD), 0);
            assert_eq!(counters1_pre_op.n_written.load(COUNTER_ORD), 0);
            assert_eq!(counters2_pre_op.n_polled.load(COUNTER_ORD), 0);
            assert_eq!(counters2_pre_op.n_transform_in.load(COUNTER_ORD), 0);
            assert_eq!(counters2_pre_op.n_transform_out.load(COUNTER_ORD), 0);
            assert_eq!(counters2_pre_op.n_written.load(COUNTER_ORD), 0);
        })
        .after_operation_begin(move |_| {
            // check pipeline startup
            assert_eq!(state1_op.get(), State::PostPipelineStart);
            assert_eq!(state2_op.get(), State::PostPipelineStart);
        });
    let agent = builder.build_and_start().expect("agent should start fine");

    // Check that the plugins have been enabled
    assert!(agent
        .initialized_plugins
        .iter()
        .find(|p| p.name() == "plugin1")
        .is_some());
    assert!(agent
        .initialized_plugins
        .iter()
        .find(|p| p.name() == "plugin2")
        .is_some());

    // Stop the pipeline
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();

    // check that the plugins are stopped
    assert_eq!(state1.get(), State::Stopped);
    assert_eq!(state2.get(), State::Stopped);

    // check that the transforms and outputs processed every measurement
    println!("counters1: {counters1:?}");
    println!("counters2: {counters2:?}");
    let total_polled = counters1.n_polled.load(COUNTER_ORD) + counters2.n_polled.load(COUNTER_ORD);
    let transform1_in = counters1.n_transform_in.load(COUNTER_ORD);
    let transform2_in = counters2.n_transform_in.load(COUNTER_ORD);
    let transform1_out = counters1.n_transform_out.load(COUNTER_ORD);
    let transform2_out = counters2.n_transform_out.load(COUNTER_ORD);
    let output1_written = counters1.n_written.load(COUNTER_ORD);
    let output2_written = counters2.n_written.load(COUNTER_ORD);
    assert_eq!(total_polled, transform1_in);
    assert_eq!(transform1_in * 2, transform1_out); // the test transform doubles the number of data points
    assert_eq!(transform1_out, transform2_in);
    assert_eq!(transform2_in * 2, transform2_out); // transform 2 runs after transform 1
    assert_eq!(transform2_out, output1_written); // each output writes every measurement
    assert_eq!(transform2_out, output2_written);
}

/// Sorts a vector of strings and returns it.
fn sorted<A: AsRef<str> + Ord>(mut strings: Vec<A>) -> Vec<A> {
    strings.sort();
    strings
}

struct MyPlugin;
impl AlumetPlugin for MyPlugin {
    fn name() -> &'static str {
        "name"
    }

    fn version() -> &'static str {
        "version"
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        todo!()
    }

    fn start(&mut self, _alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(MyPluginConfig::default())?;
        Ok(Some(config))
    }
}

#[derive(Serialize)]
struct MyPluginConfig {
    list: Vec<String>,
    count: u32,
}

impl Default for MyPluginConfig {
    fn default() -> Self {
        Self {
            list: vec![String::from("default-item")],
            count: 42,
        }
    }
}

#[derive(Serialize)]
struct MyAgentConfig {
    global_setting: String,
}

impl Default for MyAgentConfig {
    fn default() -> Self {
        Self {
            global_setting: String::from("default"),
        }
    }
}
