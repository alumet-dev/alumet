//! This file contains tests for the testing module.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use alumet::{
    agent::{self, plugin::PluginSet},
    measurement::{
        MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementType, WrappedMeasurementValue,
    },
    metrics::{Metric, TypedMetricId},
    pipeline::{
        elements::{error::PollError, source::trigger::TriggerSpec},
        error::PipelineError,
        naming::{ElementName, SourceName},
        Source,
    },
    plugin::rust::AlumetPlugin,
    resources::{Resource, ResourceConsumer},
    static_plugins,
    units::Unit,
};

const TIMEOUT: Duration = Duration::from_secs(2);

struct TestedPlugin;
struct CoffeeSource {
    metric: TypedMetricId<u64>,
}

// In the tests, we use this static to simulate data that comes from an external environment.
static COUNT: AtomicU64 = AtomicU64::new(0);

impl AlumetPlugin for TestedPlugin {
    fn name() -> &'static str {
        "plugin"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let counter = alumet.create_metric::<u64>(
            "coffee_counter",
            Unit::Unity,
            "count the number of coffees that were consumed during development",
        )?;
        alumet.add_source(
            "coffee_source",
            Box::new(CoffeeSource { metric: counter }),
            TriggerSpec::at_interval(Duration::from_secs(1)),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl Source for CoffeeSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, t: Timestamp) -> Result<(), PollError> {
        measurements.push(MeasurementPoint::new(
            t,
            self.metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            COUNT.load(Ordering::Relaxed),
        ));
        Ok(())
    }
}

fn init_logger() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
}

#[test]
fn source_assert_error() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = alumet::test::RuntimeExpectations::new().source_result(
        SourceName::from_str("plugin", "coffee_source"),
        || {
            // Prepare the environment (here simulated by a static variable) for the test.
            log::debug!("preparing input for test");
            COUNT.store(27, Ordering::Relaxed);
        }, // the test module takes care of triggering the source
        |m| {
            // The source has been triggered by the test module, check its output.
            log::debug!("checking output for test");
            assert_eq!(m.len(), 1);
            let measurement = m.iter().next().unwrap();
            assert_eq!(measurement.value, WrappedMeasurementValue::U64(28));
        },
    );

    let expectations = alumet::test::StartupExpectations::default()
        .expect_metric(Metric {
            name: String::from("coffee_counter"),
            description: String::new(),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Unity.into(),
        })
        .expect_source("plugin", "coffee_source");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    std::thread::sleep(Duration::from_secs(1));
    agent.pipeline.control_handle().shutdown(); // TODO don't do this, shutdown after the tests are all done!
    let res = agent.wait_for_shutdown(TIMEOUT);
    let err = res.expect_err("the source should fail and the error should be propagated");
    let element_name = err
        .downcast_ref::<PipelineError>()
        .expect("the last and only error should be a PipelineError")
        .element()
        .expect("the PipelineError should originate from a source");
    assert_eq!(
        element_name,
        &ElementName::from(SourceName::new("plugin".into(), "coffee_source".into()))
    );
}

#[test]
fn source_assert_ok() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = alumet::test::RuntimeExpectations::new().source_result(
        SourceName::from_str("plugin", "coffee_source"),
        || {
            // Prepare the environment (here simulated by a static variable) for the test.
            log::debug!("preparing input for test");
            COUNT.store(27, Ordering::Relaxed);
        }, // the test module takes care of triggering the source
        |m| {
            // The source has been triggered by the test module, check its output.
            log::debug!("checking output for test");
            assert_eq!(m.len(), 1);
            let measurement = m.iter().next().unwrap();
            assert_eq!(measurement.value, WrappedMeasurementValue::U64(27));
        },
    );

    let expectations = alumet::test::StartupExpectations::default()
        .expect_metric(Metric {
            name: String::from("coffee_counter"),
            description: String::new(),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Unity.into(),
        })
        .expect_source("plugin", "coffee_source");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
