use std::sync::{Arc, Mutex};

use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    plugin::rust::{serialize_config, AlumetPlugin},
    plugin::{AlumetPreStart, ConfigTable},
    units::Unit,
};

use serde::{Deserialize, Serialize};

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

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(_: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(EnergyAttributionPlugin {
            metrics: Arc::new(Mutex::new(Metrics::default())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
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

    fn pre_pipeline_start(&mut self, alumet: &mut AlumetPreStart) -> anyhow::Result<()> {
        /// Finds the RawMetricId with the name of the metric.
        /// Will only run once, just before the pipeline starts.
        fn find_metric_by_name(alumet: &mut AlumetPreStart, name: &str) -> anyhow::Result<RawMetricId> {
            let (id, _metric) = alumet
                .metrics()
                .into_iter()
                .find(|m| m.1.name == name)
                .unwrap_or_else(|| panic!("Cannot find metric {name}, are the 'rapl' and 'k8s' plugins loaded?"));
            Ok(id.to_owned())
        }

        // Lock the metrics mutex to apply its modifications.
        let mut metrics = self.metrics.lock().unwrap();

        metrics.rapl_consumed_energy = Some(find_metric_by_name(alumet, "rapl_consumed_energy")?);
        metrics.cpu_usage_per_pod = Some(find_metric_by_name(alumet, "cgroup_cpu_usage_user")?);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Config {
    oui: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            oui: String::from("oui"),
        }
    }
}
