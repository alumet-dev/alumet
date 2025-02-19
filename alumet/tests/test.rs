//! This file is for testing test module.
//!
//! This test show how to test your plugin with a better code coverage about its metrics, plugins,...
//!
//! # Examples
//!
//! ```
//! use std::time::Duration;
//! use alumet::{agent, measurement::MeasurementPoint, metrics::Metric, static_plugins};
//!
//! const TIMEOUT: Duration = Duration::from_secs(2);
//!
//! #[test]
//! fn plugin_in_pipeline() {
//!     struct TestedPlugin;
//!
//!     let tester = alumet::test::RuntimeExpectations::new() // Create a RuntimeExpectations structure
//!         .source_output("tested/source/1", |m| {     // Add a new source_output to check its output
//!             assert_eq!(m.len(), 2);                 // Check if the measurement buffer's size is 2
//!             assert_eq!(m[0].value, 123.5);          // Check if the first value is 123.5
//!         })
//!         .transform_result("t1", || {                    // Add a new transform_result to check
//!             let mut input = MeasurementBuffer::new();   // Create the input data for the transform plugin
//!             input.push(MeasurementPoint::new(...);
//!             // ...
//!             (input, MeasurementOrigin::Source(rapl_source_id))
//!         }, |output| {assert_eq!(output, ...)})          // Check if the ouput is correct depending on input value above
//!         .build();
//!     
//!     let mut plugins = static_plugins![TestedPlugin]; // Add our plugin to the agent
//!     
//!     let mut plugins = agent::plugin::PluginSet::new(plugins); // Create the associated PluginSet for plugins
//!     
//!     let expectations = alumet::test::StartupExpectations::default() // Create a StartupExpectations structure
//!         .start_metric( Metric { name: todo!(), description: todo!(), value_type: todo!(), unit: todo!() }) // Adding a metric whose existence is to be verified
//!         .start_metric( Metric { name: todo!(), description: todo!(), value_type: todo!(), unit: todo!() }) // Adding a metric whose existence is to be verified
//!         .start_metric( Metric { name: todo!(), description: todo!(), value_type: todo!(), unit: todo!() }) // Adding a metric whose existence is to be verified
//!         .start_metric( Metric { name: todo!(), description: todo!(), value_type: todo!(), unit: todo!() }) // Adding a metric whose existence is to be verified
//!         .element_source("source1", SourceType::Managed) // Adding a source, defined by its name whose existence is to be verified
//!         .element_transform("tron"); // Adding a transform, defined by its name whose existence is to be verified
//!
//!     // The agent is created using both defined above structures
//!     let agent = agent::Builder::new(plugins)
//!         .with_expectations(expectations)    // Add the StartupExpectations structure
//!         .with_tester(tester)                // Add the RuntimeExpectations structure
//!         .build_and_start()
//!         .expect("startup failure");
//!     
//!     agent.wait_for_shutdown(TIMEOUT).unwrap();
//! }
//! ```

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
    pipeline::{elements::error::PollError, elements::source::trigger::TriggerSpec, naming::SourceName, Source},
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
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).try_init();
}

#[test]
#[should_panic]
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

    agent.pipeline.control_handle().shutdown();
    agent.wait_for_shutdown(TIMEOUT).unwrap();
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
