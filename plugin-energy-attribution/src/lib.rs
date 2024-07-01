use std::sync::{Arc, Mutex};

use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    pipeline::runtime::IdlePipeline,
    plugin::rust::AlumetPlugin,
    units::Unit,
};

use anyhow::Context;
use transform::EnergyAttributionTransform;

mod transform;

pub struct EnergyAttributionPlugin {
    metrics: Arc<Mutex<Metrics>>,
}

#[derive(Default)]
struct Metrics {
    // To attribute the CPU consumption to K8S pods, we need three metrics:
    // - cpu usage per pod
    // - hardware cpu consumption
    // - energy attribution (to store the result)
    //
    // Their IDs are gathered in different phases of the plugin initialization,
    // that is why they are Options.
    cpu_usage_per_pod: Option<RawMetricId>,
    rapl_consumed_energy: Option<RawMetricId>,
    pod_attributed_energy: Option<TypedMetricId<f64>>,
}

impl AlumetPlugin for EnergyAttributionPlugin {
    fn name() -> &'static str {
        "energy-attribution"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(_: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(EnergyAttributionPlugin {
            metrics: Arc::new(Mutex::new(Metrics::default())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let mut metrics = self.metrics.lock().unwrap();

        // Create the energy attribution metric and add its id to the
        // transform plugin metrics' list.
        metrics.pod_attributed_energy = Some(alumet.create_metric(
            "pod_attributed_energy",
            Unit::Joule,
            "Energy consumption attributed to the pod",
        )?);

        // Add the transform now but fill its metrics later.
        alumet.add_transform(Box::new(EnergyAttributionTransform::new(self.metrics.clone())));
        Ok(())
    }

    fn pre_pipeline_start(&mut self, pipeline: &IdlePipeline) -> anyhow::Result<()> {
        /// Finds the RawMetricId with the name of the metric.
        /// Will only run once, just before the pipeline starts.
        fn find_metric_by_name(pipeline: &IdlePipeline, name: &str) -> anyhow::Result<RawMetricId> {
            let (id, _metric) = pipeline
                .metric_iter()
                .find(|m| m.1.name == name)
                .with_context(|| format!("Cannot find metric {name}, is the 'rapl' plugin loaded?"))?;
            Ok(id.to_owned())
        }

        // Lock the metrics mutex to apply its modifications.
        let mut metrics = self.metrics.lock().unwrap();

        metrics.rapl_consumed_energy = Some(find_metric_by_name(pipeline, "rapl_consumed_energy")?);
        metrics.cpu_usage_per_pod = Some(find_metric_by_name(pipeline, "total_usage_usec")?);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
