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

use alumet::{agent, measurement::MeasurementPoint, metrics::Metric, static_plugins};


const TIMEOUT: Duration = Duration::from_secs(2);

#[test]
fn plugin_in_pipeline() {
    struct TestedPlugin;

    let tester = alumet::test::RuntimeExpectations::new()
        // TODO expliquer comment trouver le nom des sources automatiques
        .source_output("tested/source/1", |m| {
            assert_eq!(m.len(), 2);
            assert_eq!(m[0].value, 123.5);
        })
        .transform_result("t1", || {
            let mut input = MeasurementBuffer::new();
            input.push(MeasurementPoint::new(...);
            // ...
            (input, MeasurementOrigin::Source(rapl_source_id))
        }, |output| {assert_eq!(output, ...)})
        .build();
    
    let mut plugins = static_plugins![TestedPlugin];
    
    let mut plugins = agent::plugin::PluginSet::new(plugins);
    
    let expectations = alumet::test::StartupExpectations::default()
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .start_metric( Metric { ... })
        .element_source("source1", SourceType::Managed)
        .element_transform("tron");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_tester(tester)
        .build_and_start()
        .expect("startup failure");
    
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}