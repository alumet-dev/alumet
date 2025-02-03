use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    pipeline::elements::transform::builder::TransformRegistration,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use anyhow::Context;

use transform::EnergyEstimationTdpTransform;

mod transform;

pub struct EnergyEstimationTdpPlugin {
    config: Option<Config>,
}

struct Metrics {
    //todo : add comment about new metrics

    system_cpu_usage: RawMetricId,
    system_estimated_energy_consumption: TypedMetricId<f64>,
}

impl AlumetPlugin for EnergyEstimationTdpPlugin {
    // So we define the name of the plugin.
    fn name() -> &'static str {
        "EnergyEstimationTdp"
    }

    // We also define it's version.
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    // We use the default config by default and on initialization.
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(EnergyEstimationTdpPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Create the energy attribution metric and add its id to the
        // transform plugin metrics' list.
        let system_estimated_energy = alumet.create_metric(
            "system_estimated_energy",
            Unit::Joule,
            "System's estimated energy consumption",
        )?;

        let cpu_usage = self.config.as_ref().unwrap().system_cpu_usage.clone();
        let config_cpy = self.config.take().unwrap();

        // Add the transform now but fill its metrics later.
        alumet.add_transform_builder(move |ctx| {
            let name = ctx.transform_name("energy_estimation_tdp");

            let cpu_usage_metric = ctx
                .metric_by_name(&cpu_usage)
                .with_context(|| format!("Metric not found : {}", cpu_usage))?
                .0;
            let metrics = Metrics {
                system_cpu_usage: cpu_usage_metric,
                system_estimated_energy_consumption: system_estimated_energy,
            };

            let transform = Box::new(EnergyEstimationTdpTransform::new(config_cpy, metrics));
            Ok(TransformRegistration { name, transform })
        });
        Ok(())
    }

    // The stop function is called after all the metrics, sources and output previously
    // registered have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

// for 1st version, tdp,vcpu, cpu are defined in configuration plugin
#[derive(Serialize, Deserialize)]
struct Config {
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    tdp: f64,
    nb_vcpu: f64,
    nb_cpu: f64,
    system_cpu_usage: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            tdp: 100.0,
            nb_vcpu: 16.0,
            nb_cpu: 16.0,
            system_cpu_usage: String::from("kernel_cpu_time"),
        }
    }
}
