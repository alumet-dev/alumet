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

use std::time::Duration;

use alumet::{agent::{self, plugin::PluginSet}, measurement::{MeasurementPoint, Timestamp}, metrics::Metric, pipeline::naming::SourceName, plugin::rust::AlumetPlugin, static_plugins};


const TIMEOUT: Duration = Duration::from_secs(2);

struct TestedPlugin;

impl AlumetPlugin for TestedPlugin {
    fn name() -> &'static str {
        "tested"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn plugin_in_pipeline() {
    let mut plugins = PluginSet::from(static_plugins![TestedPlugin]);

    let runtime = alumet::test::RuntimeExpectations::new()
        // Test des sources :
        // - modifier l'environnement
        // - trigger manuel ou périodique
        // - source.poll() -> mesures
        // - on vérifie les mesures
        .source_result(SourceName::from_str("tested", "s1"), 
            || {
                // modifier l'environnement
                todo!()
            }
            , // trigger par le module de test
            |m| {
                // vérification du résultat
                assert_eq!(m.len(), 2);
                assert_eq!(m[0].value, 123.5);
            }
        )
        .transform_result("t1", |ctx| {
            // création de l'entrée
            let mut input = MeasurementBuffer::new();
            let t = Timestamp::now();
            let metric1 = ctx.metrics().by_name("rapl_consumed_energy").unwrap();
            let metric2 = ctx.metrics().by_name("rapl_max_power").unwrap();
            input.push(MeasurementPoint::new(t, metric1, ...));
            input.push(MeasurementPoint::new(t, metric2, ...));
            // ...
            input
        }, |output| {
            // vérification du résultat
            assert_eq!(output, ...)
        })
        .output_result("out", |ctx| {
            // création de l'entrée
            let mut input = MeasurementBuffer::new();
            todo!();
            input
        }, |output| {
            // vérification de la sortie
            todo!()
        })
        .build();
        
    let expectations = alumet::test::StartupExpectations::default()
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .element_source("source1", SourceType::Managed)
        .element_transform("tron");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_expectations(runtime)
        .build_and_start()
        .expect("startup failure");
    
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
