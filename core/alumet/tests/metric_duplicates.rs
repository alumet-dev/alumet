use alumet::{
    measurement::WrappedMeasurementType,
    metrics::{Metric, duplicate::DuplicateReaction},
    plugin::rust::AlumetPlugin,
    units::Unit,
};

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

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        let m1 = Metric {
            name: "m1".to_owned(), // existing metric, but compatible
            description: "".to_owned(),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Second.into(),
        };
        let m2 = Metric {
            name: "m2".to_owned(),
            description: "".to_owned(),
            value_type: WrappedMeasurementType::U64, // bad
            unit: Unit::Second.into(),
        };
        let m3 = Metric {
            name: "m3".to_owned(),
            description: "".to_owned(),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Second.into(), // bad
        };
        let m4 = Metric {
            name: "m4".to_owned(), // new metric
            description: "".to_owned(),
            value_type: WrappedMeasurementType::F64,
            unit: Unit::Watt.into(),
        };

        // Attempt to create these 4 metrics. Only m2 and m3 should succeed.
        let metrics = vec![m1, m2, m3, m4];
        let res = alumet
            .block_on(
                alumet
                    .metrics_sender()
                    .create_metrics(metrics, DuplicateReaction::Error),
            )
            .expect("metrics_sender().create_metrics should send the message");

        assert_eq!(4, res.len(), "there should be one result per metric");
        assert!(res[0].is_ok());
        assert!(res[1].as_ref().is_err_and(|e| e.name == "m2"));
        assert!(res[2].as_ref().is_err_and(|e| e.name == "m3"));
        assert!(res[3].is_ok());
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
