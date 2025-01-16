use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    pipeline::elements::transform::builder::TransformRegistration,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};

use anyhow::Context;

use serde::{Deserialize, Serialize};

use transform::EnergyAttributionTransform;

mod transform;

pub struct EnergyAttributionPlugin {
    config: Config,
}

struct Metrics {
    // To attribute the CPU consumption to K8S pods, we need three metrics:
    // - overall consumed energy per pod
    // - overall hardware usage per pod
    // - energy attributed to a pod
    //
    // Their IDs are gathered in different phases of the plugin initialization,
    // that is why they are Options.
    hardware_usage: RawMetricId,
    consumed_energy: RawMetricId,
    pod_attributed_energy: TypedMetricId<f64>,
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

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(EnergyAttributionPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Create the energy attribution metric and add its id to the
        // transform builder's metrics list.
        let attribution_energy_metric = alumet.create_metric(
            "pod_attributed_energy",
            Unit::Joule,
            "Energy consumption attributed to the pod",
        )?;

        let consumed_energy = self.config.consumed_energy_rapl.clone();
        let hardware_usage = self.config.hardware_usage_cgroup.clone();

        // Add the transform builder and its metrics
        alumet.add_transform_builder(move |ctx| {
            let name = ctx.transform_name("attribution_transform");

            let consumed_energy_metric = ctx
                .metric_by_name(&consumed_energy)
                .with_context(|| format!("Metric not found : {}", consumed_energy))?
                .0;
            let hardware_usage_metric = ctx
                .metric_by_name(&hardware_usage)
                .with_context(|| format!("Metric not found {}", hardware_usage))?
                .0;
            let metrics = Metrics {
                pod_attributed_energy: attribution_energy_metric,
                consumed_energy: consumed_energy_metric,
                hardware_usage: hardware_usage_metric,
            };

            let transform = Box::new(EnergyAttributionTransform::new(metrics));
            Ok(TransformRegistration { name, transform })
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Config {
    consumed_energy_rapl: String,
    hardware_usage_cgroup: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            consumed_energy_rapl: String::from("rapl_consumed_energy"),
            hardware_usage_cgroup: String::from("cgroup_cpu_usage_user"),
        }
    }
}
