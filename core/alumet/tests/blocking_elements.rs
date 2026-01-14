use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{self, Duration},
};

use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::{MeasurementAccumulator, Timestamp},
    pipeline::{
        self, Source,
        control::request,
        elements::{error::PollError, source::trigger::TriggerSpec},
    },
    plugin::{
        AlumetPluginStart, AlumetPostStart, ConfigTable, PluginMetadata,
        rust::{AlumetPlugin, serialize_config},
    },
    static_plugins,
};
use anyhow::Context;
use pretty_assertions::assert_eq;

struct TestPlugin {
    counters: Arc<Counters>,
}

#[derive(Default)]
struct Counters {
    quick_polls: AtomicUsize,
    slow_polls: AtomicUsize,
}

impl TestPlugin {
    fn metadata_with(counters: Arc<Counters>) -> PluginMetadata {
        PluginMetadata {
            name: Self::name().to_owned(),
            version: Self::version().to_owned(),
            init: Box::new(move |_| Ok(Box::new(Self { counters }))),
            default_config: Box::new(Self::default_config),
        }
    }
}

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "blocking_elements"
    }

    fn version() -> &'static str {
        "0.0.1"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Duration::from_secs(1))?;
        Ok(Some(config))
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        unreachable!()
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        alumet.add_source(
            "quick",
            Box::new(QuickSource {
                counters: Arc::clone(&self.counters),
            }),
            TriggerSpec::at_interval(Duration::from_millis(10)),
        )?;
        alumet.add_blocking_source(
            "slow",
            Box::new(SlowSource {
                counters: Arc::clone(&self.counters),
            }),
            TriggerSpec::at_interval(Duration::from_millis(100)),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        Ok(())
    }
}

struct QuickSource {
    counters: Arc<Counters>,
}
struct SlowSource {
    counters: Arc<Counters>,
}

impl Source for QuickSource {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        self.counters.quick_polls.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl Source for SlowSource {
    fn poll(&mut self, _m: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        self.counters.slow_polls.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(99));
        Ok(())
    }
}

#[test]
fn blocking_elements_should_not_block_the_pipeline() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();

    // Init shared counters.
    let counters = Arc::new(Counters::default());

    // Create an agent with the plugin
    let plugins = vec![TestPlugin::metadata_with(Arc::clone(&counters))];
    let plugins = PluginSet::from(plugins);

    let mut pipeline_builder = pipeline::Builder::new();
    // Use only one thread, so that scheduling issues will block the pipeline.
    pipeline_builder.normal_threads(1);

    let agent_builder = agent::Builder::from_pipeline(plugins, pipeline_builder);

    // Start Alumet
    let agent = agent_builder.build_and_start().expect("agent should start fine");

    // Wait a little bit
    thread::sleep(time::Duration::from_secs(1));

    // Stop Alumet
    agent.pipeline.control_handle().shutdown();

    // Ensure that Alumet has stopped in less than x seconds
    let timeout_duration = Duration::from_secs(1);
    agent
        .wait_for_shutdown(timeout_duration)
        .context("error while shutting down")?;

    // Check the counters.
    let slow_polls = counters.slow_polls.load(Ordering::Relaxed);
    let quick_polls = counters.quick_polls.load(Ordering::Relaxed);
    assert!(slow_polls.abs_diff(10) <= 2); // slow source should be called approx. 10 times
    assert!(quick_polls.abs_diff(100) <= 10); // quick source should be called approx. 100 times
    Ok(())
}
