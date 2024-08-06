
use alumet::{agent::{AgentBuilder, AgentConfig}, measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp}, metrics::TypedMetricId, pipeline::{elements::error::PollError, trigger::TriggerSpec}, plugin::{rust::{deserialize_config, serialize_config, AlumetPlugin}, AlumetPluginStart, AlumetPostStart, ConfigTable}, resources::{Resource, ResourceConsumer}, static_plugins, units::{PrefixedUnit, Unit}};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{thread, time::{self, Duration}};

pub struct Metrics {
    pub time_tot: TypedMetricId<u64>,
}

pub struct MyTestPluginLateMetricCreation {
    config: Config,
    metrics: Option<Metrics>,
}

#[derive(Debug)]
struct MyTestSourcePlugin {
    value: TypedMetricId<u64>,
}

#[derive(Deserialize, Serialize)]
struct Config {
    /// Initial interval between two cgroup measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl AlumetPlugin for MyTestPluginLateMetricCreation {
    fn name() -> &'static str {
        "late_source_creation"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(MyTestPluginLateMetricCreation {
            config,
            metrics: None,
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let usec: PrefixedUnit = PrefixedUnit::micro(Unit::Second);
        let usec_metric = alumet.create_metric("A",usec, "A random metric to test late metric creation inside post_pipeline_start")?;
        self.metrics = Some(Metrics {
            time_tot: usec_metric,
        });
        // No source creation in the start function
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        let probe = MyTestSourcePlugin{
            value:(self.metrics.as_ref().expect("Can't read byte_metric")).time_tot,
        };

        // Add the probe to the sources
        control_handle
            .add_source(
                "x",
                Box::new(probe),
                TriggerSpec::at_interval(self.config.poll_interval),
            )
            .with_context(|| format!("failed to add source when testing add source in post_pipeline_start"))?;
        Ok(())                         
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
        }
    }
}

impl alumet::pipeline::Source for MyTestSourcePlugin {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let consumer = ResourceConsumer::LocalMachine;
        let p_tot: MeasurementPoint = MeasurementPoint::new(
            timestamp,
            self.value,
            Resource::LocalMachine,
            consumer.clone(),
            1,
        );
        measurements.push(p_tot);
        Ok(())
    }
}


#[test]
fn late_source_creation_test() {
    /* This function test for a source add in the post_pipeline_start function */
    let plugins = static_plugins![MyTestPluginLateMetricCreation];
    
    let agent = AgentBuilder::new(plugins)
        .config_value(toml::Table::new())
        .build();

    // Stop the pipeline
    let global_config = agent.default_config().unwrap();
    let agent_config = AgentConfig::try_from(global_config).unwrap();
    let agent = agent.start(agent_config).unwrap();
    thread::sleep(time::Duration::from_secs(3));
    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown().unwrap();

}