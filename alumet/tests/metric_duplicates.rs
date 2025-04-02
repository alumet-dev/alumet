use alumet::{plugin::rust::AlumetPlugin, units::Unit};

struct TestPlugin;

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "test"
    }

    fn version() -> &'static str {
        "0"
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        alumet.create_metric::<u64>("m1", Unit::Second, "test metric")?;
        alumet.create_metric::<u64>("m1", Unit::Second, "test metric")?;
        alumet
            .create_metric::<f64>("m1", Unit::Second, "test metric")
            .expect_err("incompatible metric registration should fail");
        alumet
            .create_metric::<u64>("m1", Unit::Watt, "test metric")
            .expect_err("incompatible metric registration should fail");
        alumet.create_metric::<f64>("m2", Unit::Second, "test metric 2, different from test metric 1")?;
        alumet.create_metric::<u64>("m3", Unit::Watt, "test metric")?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(feature = "test")]

mod tests {
    use std::time::Duration;

    use alumet::{agent::plugin::PluginSet, static_plugins, test::StartupExpectations, units::Unit};

    #[test]
    fn plugin_metric_registration() {
        const TIMEOUT: Duration = Duration::from_millis(250);
        let plugins = PluginSet::from(static_plugins![super::TestPlugin]);
        let expectations = StartupExpectations::new()
            .expect_metric::<u64>("m1", Unit::Second)
            .expect_metric::<f64>("m2", Unit::Second)
            .expect_metric::<u64>("m3", Unit::Watt);
        let agent = alumet::agent::Builder::new(plugins)
            .with_expectations(expectations)
            .build_and_start()
            .expect("agent should build");
        agent.pipeline.control_handle().shutdown();
        agent.wait_for_shutdown(TIMEOUT).expect("error while running");
    }
}
