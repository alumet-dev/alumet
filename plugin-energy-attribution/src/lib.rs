use std::{collections::HashMap, sync::{Arc, Mutex}};

use alumet::{
    metrics::{registry::MetricRegistry, RawMetricId, TypedMetricId},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
    units::Unit,
};

use anyhow::Context;

use serde::{Deserialize, Serialize};

use transform::EnergyAttributionTransform;

use tokio::sync::mpsc;

mod transform;

pub struct EnergyAttributionPlugin {
    config: Config,
}

struct Metrics {
    // To attribute a consumption to an entity, we need three metrics:
    // - overall consumed energy per entity
    // - overall hardware usage per entity
    // - energy attributed to the entity
    //
    // Their IDs are gathered in different phases of the plugin initialization,
    // that is why they are Options.
    hardware_usage: Option<RawMetricId>,
    global_hardware_usage: Option<RawMetricId>,
    consumed_energy: Option<RawMetricId>,
    attributed_energy: Option<TypedMetricId<f64>>,
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
            "attributed_energy",
            Unit::Joule,
            "Energy consumption attributed to the entity",
        )?;

        let consumed_energy = self.config.consumed_energy_metric_name.clone();
        let hardware_usage = self.config.hardware_usage_metric_name.clone();
        let global_hardware_usage = self.config.global_hardware_usage_metric_name.clone();
        let hardware_metric_filter = self.config.hardware_usage_metric_filter.clone().unwrap_or_default();

        let metric_reader = Arc::new(Mutex::new(Option::<MetricRegistry>::None));

        let metrics = Metrics {
            attributed_energy: Some(attribution_energy_metric),
            consumed_energy: None,
            global_hardware_usage: None,
            hardware_usage: None,
        };

        alumet.add_transform(
            "energy-attribution",
            Box::new(EnergyAttributionTransform::new(
                metrics,
                hardware_metric_filter,
                &metric_reader.clone(),
            )),
        )?;
        // // Add the transform builder and its metrics
        // alumet.add_transform_builder("transform", move |ctx| {
        //     // let consumed_energy_metric = ctx
        //     //     .metric_by_name(&consumed_energy)
        //     //     .with_context(|| format!("Metric not found : {}", consumed_energy))?
        //     //     .0;
        //     // let hardware_usage_metric = ctx
        //     //     .metric_by_name(&hardware_usage)
        //     //     .with_context(|| format!("Metric not found {}", hardware_usage))?
        //     //     .0;
        //     // let global_hardware_usage_metric = ctx
        //     //     .metric_by_name(&global_hardware_usage)
        //     //     .with_context(|| format!("Metric not found {}", global_hardware_usage))?
        //     //     .0;
        //     let metrics = Metrics {
        //         attributed_energy: Some(attribution_energy_metric),
        //         consumed_energy: None,
        //         global_hardware_usage: None,
        //         hardware_usage: None,
        //     };

        //     let transform = Box::new();
        //     Ok(transform)
        // })?; 

        let metric_reader_clone = metric_reader.clone();
        alumet.on_pre_pipeline_start( move |prestart| {
            metric_reader_clone.clone().lock().unwrap().replace(prestart.metrics().clone());
            Ok(())
        });
        // alumet.on_pre_pipeline_start(move |pre_start| {
        //     // Register existing metrics.
        //     let Some(hardware_usage) = pre_start.metrics().by_name(&self.config.clone().hardware_usage_metric_name.clone());

        //     Ok(())
        // });

        // alumet.on_pre_pipeline_start(move |pre_start| {
        //     pre_start.add_metric_listener("a", move |new_metrics| {
        //         for metric in new_metrics {
        //             if metric.1.name == consumed_energy.clone() {
        //                 self.
        //             }
        //         }
        //     })?;
        // });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Clone)]
struct Config {
    consumed_energy_metric_name: String,
    global_hardware_usage_metric_name: String,
    hardware_usage_metric_name: String,
    hardware_usage_metric_filter: Option<HashMap<String, String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            consumed_energy_metric_name: String::from("rapl_consumed_energy"),
            hardware_usage_metric_name: String::from("cpu_time_delta"),
            hardware_usage_metric_filter: None,
            global_hardware_usage_metric_name: String::from("kernel_cpu_time"),
        }
    }
}
