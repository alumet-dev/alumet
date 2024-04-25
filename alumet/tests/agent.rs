use std::{env, panic};

use alumet::{
    agent::AgentBuilder,
    plugin::{
        rust::{serialize_config, AlumetPlugin},
        AlumetStart, ConfigTable,
    },
    static_plugins,
};
use serde::Serialize;

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
    let agent = AgentBuilder::new(plugins).build();
    let config = agent.default_config();

    let expected = toml::Table::from_iter(vec![(String::from("plugins"), toml::Value::Table(toml::Table::new()))]);
    assert_eq!(expected, config);
}

#[test]
fn default_config_1_plugin() {
    {
        let plugins = static_plugins![MyPlugin];
        let agent = AgentBuilder::new(plugins).build();
        let config = agent.default_config();

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
        let mut default_agent_config = toml::Table::new();
        default_agent_config.insert(String::from("key"), toml::Value::String(String::from("value")));
        let agent = AgentBuilder::new(plugins)
            .default_agent_config(default_agent_config)
            .build();
        let config = agent.default_config();

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

    fn start(&mut self, _alumet: &mut AlumetStart) -> anyhow::Result<()> {
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }

    fn default_config() -> Option<ConfigTable> {
        let config = serialize_config(MyPluginConfig::default()).unwrap();
        Some(config)
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
