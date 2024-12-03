use alumet::{
    metrics::{RawMetricId, TypedMetricId},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPreStart, ConfigTable,
    },
    units::Unit,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use transform::EnergyEstimationTdpTransform;

mod transform;

pub struct EnergyEstimationTdpPlugin {
    config: Option<Config>,
    metrics: Arc<Mutex<Metrics>>,
}

#[derive(Default)]
struct Metrics {
    // To attribute the CPU consumption to K8S pods, we need 2 metrics:
    // - cpu usage per pod
    // - energy attribution (to store the result)

    // The other parameters (tdp and number of virtual cpu is provided by configuration)
    cpu_usage_per_pod: Option<RawMetricId>,
    pod_estimate_attributed_energy: Option<TypedMetricId<f64>>,
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
        Ok(Box::new(EnergyEstimationTdpPlugin {
            config: Some(config),
            metrics: Arc::new(Mutex::new(Metrics::default())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let mut metrics = self.metrics.lock().unwrap();
        let config: Config = self.config.take().unwrap();

        // Create the energy attribution metric and add its id to the
        // transform plugin metrics' list.
        metrics.pod_estimate_attributed_energy = Some(alumet.create_metric(
            "pod_estimate_attributed_energy",
            Unit::Joule,
            "Energy consumption estimated to the pod",
        )?);

        // Add the transform now but fill its metrics later.
        alumet.add_transform(Box::new(EnergyEstimationTdpTransform::new(
            config,
            self.metrics.clone(),
        )));
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
                .expect(&format!("Cannot find metric {name}, is the 'rapl' plugin loaded?").to_string());
            Ok(id.to_owned())
        }

        // Lock the metrics mutex to apply its modifications.
        let mut metrics = self.metrics.lock().unwrap();

        metrics.cpu_usage_per_pod = Some(find_metric_by_name(alumet, "cgroup_cpu_usage_total")?);
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            tdp: 100.0,
            nb_vcpu: 1.0,
            nb_cpu: 1.0,
        }
    }
}
