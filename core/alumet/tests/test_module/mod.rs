//! This file contains tests for the testing module.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use alumet::{
    agent::{self, builder::AgentShutdownError, plugin::PluginSet},
    measurement::{
        MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType,
        WrappedMeasurementValue,
    },
    metrics::TypedMetricId,
    pipeline::{
        Output, Source, Transform,
        elements::{
            error::{PollError, TransformError, WriteError},
            output::OutputContext,
            source::trigger::TriggerSpec,
            transform::TransformContext,
        },
        naming::{ElementName, OutputName, SourceName, TransformName},
    },
    plugin::rust::AlumetPlugin,
    resources::{Resource, ResourceConsumer},
    static_plugins,
    test::{RuntimeExpectations, startup::Metric},
    units::Unit,
};
use serial_test::serial;

const TIMEOUT: Duration = Duration::from_secs(5);

struct TestedPlugin;
struct CoffeeSource {
    metric: TypedMetricId<u64>,
}

struct CoffeeTransform;
struct CoffeeOutput;

// In the tests, we use this static to simulate data that comes from an external environment.
static COUNT: AtomicU64 = AtomicU64::new(0);

// We use this static to simulate data that is exported by the output.
static OUTPUT: AtomicU64 = AtomicU64::new(0);

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
        alumet.add_transform("coffee_transform", Box::new(CoffeeTransform))?;
        alumet.add_blocking_output("coffee_output", Box::new(CoffeeOutput))?;
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
impl Transform for CoffeeTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // double the amount of coffee!
        for m in measurements.iter_mut() {
            log::trace!("transforming {m:?}");
            if let WrappedMeasurementValue::U64(v) = m.value {
                m.value = WrappedMeasurementValue::U64(v * 2);
            }
            log::trace!("after transform: {m:?}");
        }
        Ok(())
    }
}
impl Output for CoffeeOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, _ctx: &OutputContext) -> Result<(), WriteError> {
        // export the amount of coffee
        log::debug!("writing {measurements:?}");
        let last = measurements
            .iter()
            .last()
            .expect("there should be at least one measurement");
        log::debug!("last point to write: {last:?}");
        if let WrappedMeasurementValue::U64(v) = last.value {
            OUTPUT.store(v, Ordering::Relaxed);
        }
        Ok(())
    }
}

fn init_logger() {
    // Ignore errors because the logger can only be initialized once, and we run multiple tests.
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
}

#[test]
#[serial]
fn startup_ok() {
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let startup = alumet::test::StartupExpectations::new()
        .expect_metric::<u64>("coffee_counter", Unit::Unity)
        .expect_source("plugin", "coffee_source")
        .expect_output("plugin", "coffee_output")
        .expect_transform("plugin", "coffee_transform");

    let agent = agent::Builder::new(plugins)
        .with_expectations(startup)
        .build_and_start()
        .unwrap();

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
#[serial]
#[should_panic]
fn startup_bad_metric() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let startup = alumet::test::StartupExpectations::new().expect_metric_untyped(Metric {
        name: String::from("bad_metric"),
        value_type: WrappedMeasurementType::U64,
        unit: Unit::Unity.into(),
    });

    let _ = agent::Builder::new(plugins)
        .with_expectations(startup)
        .build_and_start();
}

#[test]
#[serial]
fn runtime_source_err() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_source(
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

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    let res = agent.wait_for_shutdown(TIMEOUT);
    let err = res.expect_err("the source test should fail and the error should be propagated");
    match &err.errors[..] {
        [AgentShutdownError::Pipeline(err)] => {
            let element_name = err
                .element()
                .expect("the PipelineError should originate from an element");
            assert_eq!(
                element_name,
                &ElementName::from(SourceName::new("plugin".into(), "coffee_source".into()))
            );
        }
        bad => {
            panic!("unexpected errors: {bad:?}");
        }
    }
}

#[test]
#[serial]
fn runtime_source_ok() {
    init_logger();
    // TODO make tests serialized/exclusive
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_source(
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

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
#[serial]
fn runtime_transform_err() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_transform(
        TransformName::from_str("plugin", "coffee_transform"),
        |ctx| {
            let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
            let mut m = MeasurementBuffer::new();
            m.push(MeasurementPoint::new_untyped(
                Timestamp::now(),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(5),
            ));
            m
        },
        |output| {
            let m = output.measurements();
            assert_eq!(m.len(), 1);
            let point = m.iter().nth(0).unwrap();
            const BAD: u64 = 1234;
            assert_eq!(point.value, WrappedMeasurementValue::U64(BAD), "error on purpose");
        },
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    let res = agent.wait_for_shutdown(TIMEOUT);
    let err = res.expect_err("the transform test should fail and the error should be propagated");
    match &err.errors[..] {
        [AgentShutdownError::Pipeline(err)] => {
            let element_name = err
                .element()
                .expect("the PipelineError should originate from an element");
            assert_eq!(
                element_name,
                &ElementName::from(TransformName::new("plugin".into(), "coffee_transform".into()))
            );
        }
        bad => {
            panic!("unexpected errors: {bad:?}");
        }
    }
}

#[test]
#[serial]
fn runtime_transform_ok() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_transform(
        TransformName::from_str("plugin", "coffee_transform"),
        |ctx| {
            let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
            let mut m = MeasurementBuffer::new();
            m.push(MeasurementPoint::new_untyped(
                Timestamp::now(),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(5),
            ));
            m
        },
        |output| {
            let m = output.measurements();
            assert_eq!(m.len(), 1);
            let point = m.iter().nth(0).unwrap();
            assert_eq!(point.value, WrappedMeasurementValue::U64(10), "value should be doubled");
        },
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
#[serial]
fn runtime_output_err() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_output(
        OutputName::from_str("plugin", "coffee_output"),
        |ctx| {
            let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
            let mut m = MeasurementBuffer::new();
            let test_point = MeasurementPoint::new_untyped(
                Timestamp::now(),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(11),
            );
            log::debug!("pushing {test_point:?}");
            m.push(test_point);
            m
        },
        || {
            let exported_value = OUTPUT.load(Ordering::Relaxed);
            assert_eq!(exported_value, 12); // wrong value, on purpose
        },
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    let res = agent.wait_for_shutdown(TIMEOUT);
    let err = res.expect_err("the output test should fail and the error should be propagated");
    match &err.errors[..] {
        [AgentShutdownError::Pipeline(err)] => {
            let element_name = err
                .element()
                .expect("the PipelineError should originate from an element");
            assert_eq!(
                element_name,
                &ElementName::from(OutputName::new("plugin".into(), "coffee_output".into()))
            );
        }
        bad => {
            panic!("unexpected errors: {bad:?}");
        }
    }
}

#[test]
#[serial]
fn runtime_output_ok() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new().test_output(
        OutputName::from_str("plugin", "coffee_output"),
        |ctx| {
            let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
            let mut m = MeasurementBuffer::new();
            let test_point = MeasurementPoint::new_untyped(
                Timestamp::now(),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(11),
            );
            log::debug!("pushing {test_point:?}");
            m.push(test_point);
            m
        },
        || {
            let exported_value = OUTPUT.load(Ordering::Relaxed);
            assert_eq!(exported_value, 11);
        },
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}

#[test]
#[serial]
fn all_together() {
    init_logger();
    let plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = RuntimeExpectations::new()
        .test_source(
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
        )
        .test_transform(
            TransformName::from_str("plugin", "coffee_transform"),
            |ctx| {
                let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
                let mut m = MeasurementBuffer::new();
                m.push(MeasurementPoint::new_untyped(
                    Timestamp::now(),
                    metric,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    WrappedMeasurementValue::U64(5),
                ));
                m
            },
            |output| {
                let measurements = output.measurements();
                assert_eq!(measurements.len(), 1);
                let point = measurements.iter().nth(0).unwrap();
                assert_eq!(point.value, WrappedMeasurementValue::U64(10), "value should be doubled");
                assert_eq!(
                    "coffee_counter",
                    output.metrics().by_id(&point.metric).expect("metric should exist").name,
                    "point should use the coffee_counter metric"
                );
            },
        )
        .test_output(
            OutputName::from_str("plugin", "coffee_output"),
            |ctx| {
                let metric = ctx.metrics().by_name("coffee_counter").expect("metric should exist").0;
                let mut m = MeasurementBuffer::new();
                m.push(MeasurementPoint::new_untyped(
                    Timestamp::now(),
                    metric,
                    Resource::LocalMachine,
                    ResourceConsumer::LocalMachine,
                    WrappedMeasurementValue::U64(11),
                ));
                m
            },
            || {
                let exported_value = OUTPUT.load(Ordering::Relaxed);
                assert_eq!(exported_value, 11);
            },
        );

    let expectations = alumet::test::StartupExpectations::new()
        .expect_metric_untyped(Metric {
            name: String::from("coffee_counter"),
            value_type: WrappedMeasurementType::U64,
            unit: Unit::Unity.into(),
        })
        .expect_source("plugin", "coffee_source");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
