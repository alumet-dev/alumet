use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
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
    // To report the estimated CPU consumption, we need 2 metrics:
    // - cpu usage
    // - energy estimated (to store the result)

    // The other parameters (tdp and number of virtual cpu is provided by configuration)
    cpu_usage: RawMetricId,
    estimated_energy: TypedMetricId<f64>,
}

impl AlumetPlugin for EnergyEstimationTdpPlugin {
    // So we define the name of the plugin.
    fn name() -> &'static str {
        "energy-estimation-tdp"
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
        // Create the energy estimate metric and add its id to the
        // transform plugin metrics' list.
        let estimated_energy_metric = alumet.create_metric(
            "estimated_consumed_energy",
            Unit::Joule,
            "CPU's estimated energy consumption",
        )?;

        let cpu_usage = self.config.as_ref().unwrap().cpu_usage.clone();
        let config = self.config.take().unwrap();

        // Add the transform now but fill its metrics later.
        alumet.add_transform_builder("transform", move |ctx| {
            let cpu_usage_metric = ctx
                .metric_by_name(&cpu_usage)
                .with_context(|| format!("metric not found : {}", cpu_usage))?
                .0;
            let metrics = Metrics {
                cpu_usage: cpu_usage_metric,
                estimated_energy: estimated_energy_metric,
            };

            let transform = Box::new(EnergyEstimationTdpTransform::new(config, metrics));
            Ok(transform)
        })?;
        Ok(())
    }

    // The stop function is called after all the metrics, sources and output previously
    // registered have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

// for 1st version, tdp, vcpu, cpu are defined in configuration plugin
#[derive(Serialize, Deserialize)]
struct Config {
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    tdp: f64,
    nb_vcpu: f64,
    nb_cpu: f64,
    cpu_usage: String,
    cpu_time_conversion_factor: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5), // aligned with input cpu_usage metric poll_interval
            tdp: 100.0,
            nb_vcpu: 1.0,
            nb_cpu: 1.0,
            cpu_usage: String::from("kernel_cpu_time"),
            cpu_time_conversion_factor: 1000.0,
        }
    }
}
