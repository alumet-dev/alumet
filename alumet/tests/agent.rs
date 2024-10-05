use std::{sync::Arc, time::Duration};

use alumet::{
    agent, pipeline,
    plugin::{
        rust::{serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable, PluginMetadata,
    },
    static_plugins,
};
use serde::Serialize;

mod common;
use common::test_plugin::{AtomicState, State, TestPlugin};

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
    let plugins = static_plugins![];
    let mut config = toml::Table::new();
    agent::config::insert_default_plugin_configs(&plugins, &mut config).unwrap();

    let expected = toml::Table::from_iter(vec![(String::from("plugins"), toml::Value::Table(toml::Table::new()))]);
    assert_eq!(expected, config);
}

#[test]
fn default_config_1_plugin() {
    {
        let plugins = static_plugins![MyPlugin];
        let mut config = toml::Table::new();
        agent::config::insert_default_plugin_configs(&plugins, &mut config).unwrap();

        let mut expected = toml::Table::new();
        let mut expected_plugins_confs = toml::Table::new();
        let mut expected_plugin_config = toml::Table::new();
        expected_plugin_config.insert(
            String::from("list"),
            toml::Value::Array(vec![toml::Value::String(String::from("default-item"))]),
        );
        expected_plugin_config.insert(String::from("count"), toml::Value::Integer(42));
        expected_plugins_confs.insert(String::from("name"), toml::Value::Table(expected_plugin_config));
        expected.insert(String::from("plugins"), toml::Value::Table(expected_plugins_confs));
        assert_eq!(config, expected);
    }
    {
        let plugins = static_plugins![MyPlugin];
        let mut config = toml::Table::new();
        config.insert(String::from("key"), toml::Value::String(String::from("value")));

        agent::config::insert_default_plugin_configs(&plugins, &mut config).unwrap();

        let mut expected = toml::Table::new();
        let mut expected_plugins_confs = toml::Table::new();
        let mut expected_plugin_config = toml::Table::new();
        expected_plugin_config.insert(
            String::from("list"),
            toml::Value::Array(vec![toml::Value::String(String::from("default-item"))]),
        );
        expected_plugin_config.insert(String::from("count"), toml::Value::Integer(42));
        expected_plugins_confs.insert(String::from("name"), toml::Value::Table(expected_plugin_config));
        expected.insert(String::from("plugins"), toml::Value::Table(expected_plugins_confs));
        expected.insert(String::from("key"), toml::Value::String(String::from("value")));
        assert_eq!(config, expected);
    }
}

#[test]
fn test_plugin_lifecycle() {
    // Create two TestPlugins with a different name.
    // Each plugin will register 2 metrics, 1 source, 1 transform, 1 output.
    let state1 = Arc::new(AtomicState::new(State::PreInit));
    let state2 = Arc::new(AtomicState::new(State::PreInit));

    let (state1_meta, state2_meta) = (state1.clone(), state2.clone());
    let plugins = vec![
        PluginMetadata {
            name: "plugin1".to_owned(),
            version: "0.0.1".to_owned(),
            init: Box::new(move |_| Ok(TestPlugin::init("plugin1", 98, state1_meta))),
            default_config: Box::new(|| Ok(None)),
        },
        PluginMetadata {
            name: "plugin2".to_owned(),
            version: "0.0.1".to_owned(),
            init: Box::new(move |_| Ok(TestPlugin::init("plugin2", 1000, state2_meta))),
            default_config: Box::new(|| Ok(None)),
        },
    ];

    let (state1_init, state2_init) = (state1.clone(), state2.clone());
    let (state1_start, state2_start) = (state1.clone(), state2.clone());
    let (state1_pre_op, state2_pre_op) = (state1.clone(), state2.clone());
    let (state1_op, state2_op) = (state1.clone(), state2.clone());

    let pipeline_builder = pipeline::Builder::new();
    let mut builder = agent::Builder::new(pipeline_builder);
    builder
        .add_plugins(plugins)
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

    // check the plugins
    assert_eq!(state1.get(), State::Stopped);
    assert_eq!(state2.get(), State::Stopped);
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
