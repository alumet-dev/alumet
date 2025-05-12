use std::time::Duration;

use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::{PrefixedUnit, Unit},
};

use anyhow::Context;

use serde::{Deserialize, Serialize};

use transform::EnergyAttributionTransform;

mod transform;

pub struct EnergyAttributionPlugin {
    config: Config,
}

struct Metrics {
    hardware_usage: RawMetricId,
    hardware_usage_unit: PrefixedUnit,
    hardware_usage_poll_interval: Duration,
    consumed_energy: RawMetricId,
    attributed_energy: TypedMetricId<f64>,
    filter_energy_attr: Option<(String, String)>,
}

impl AlumetPlugin for EnergyAttributionPlugin {
    fn name() -> &'static str {
        "energy-attribution"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION") // To attribute the CPU consumption to K8S pods or processes, we need three metrics:
                                  // - overall consumed energy per pod
                                  // - overall hardware usage per pod
                                  // - energy attributed to a pod
                                  //
                                  // Their IDs are gathered in different phases of the plugin initialization,
                                  // that is why they are Options.
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(EnergyAttributionPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        log::warn!("Implementation restriction: the attribution plugin only works when the 'energy source' and 'usage source' are configured at a frequency <= 1 Hz, i.e. a poll_interval >= 1s");
        log::warn!("To get a correct attribution, you must set the value hardware_usage_poll_interval to the same value as the poll_interval of the usage source (for example, to attribute the consumption of processes, copy the value of plugins.procfs.processes.poll_interval)");

        // Create the energy attribution metric and add its id to the
        // transform builder's metrics list.
        let attributed_energy = alumet.create_metric(
            "attributed_energy",
            Unit::Joule,
            "Energy consumption attributed to the process or pod (ratio power model)",
        )?;

        let consumed_energy = self.config.hardware_consumption.clone();
        let hardware_usage = self.config.hardware_usage.clone();
        let filter_energy_attr = self.config.filter_energy_attr.clone();
        let hardware_usage_poll_interval = self.config.hardware_usage_poll_interval.clone();

        // Add the transform builder and its metrics
        alumet.add_transform_builder("transform", move |ctx| {
            let consumed_energy = ctx
                .metric_by_name(&consumed_energy)
                .with_context(|| format!("Metric not found : {}", consumed_energy))?
                .0;
            let (hardware_usage, usage_metric) = ctx
                .metric_by_name(&hardware_usage)
                .with_context(|| format!("Metric not found {}", hardware_usage))?;
            let hardware_usage_unit = usage_metric.unit.clone();
            let metrics = Metrics {
                hardware_usage,
                hardware_usage_unit,
                hardware_usage_poll_interval,
                consumed_energy,
                attributed_energy,
                filter_energy_attr,
            };

            let transform = Box::new(EnergyAttributionTransform::new(metrics));
            Ok(transform)
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
struct Config {
    hardware_consumption: String,
    hardware_usage: String,
    #[serde(with = "humantime_serde")]
    hardware_usage_poll_interval: Duration,
    filter_energy_attr: Option<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hardware_consumption: String::from("rapl_consumed_energy"),
            hardware_usage: String::from("cpu_time_delta"),
            hardware_usage_poll_interval: Duration::from_secs(2),
            filter_energy_attr: Some((String::from("domain"), String::from("package"))),
        }
    }
}
