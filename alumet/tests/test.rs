//! This file is for testing test module.
//! 
//! 

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
        .element_source("source1", SourceType::Managed)
        .element_transform("tron");

    let agent = agent::Builder::new(plugins)
        .with_expectations(expectations)
        .with_tester(tester)
        .build_and_start()
        .expect("startup failure");
    
    agent.wait_for_shutdown(TIMEOUT).unwrap();
}