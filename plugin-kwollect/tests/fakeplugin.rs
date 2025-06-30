use std::time::Duration;

use alumet::pipeline::elements::source::trigger;
use alumet::{
    plugin::{rust::AlumetPlugin, AlumetPluginStart, ConfigTable},
    units::Unit,
};

pub struct TestsPlugin;

impl AlumetPlugin for TestsPlugin {
    fn name() -> &'static str {
        "fackplugin"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(TestsPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let _counter_metric = alumet.create_metric::<u64>(
            "example_counter",
            Unit::Unity,
            "number of times the example source has been called", // description
        )?;
        let _trigger = trigger::builder::time_interval(Duration::from_secs(1)).build()?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
