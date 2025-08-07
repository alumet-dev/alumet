use alumet::{
    metrics::RawMetricId,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use transform::ProcessToCgroupBridgeTransform;

#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
mod tests;

mod transform;

pub struct ProcessToCgroupBridgePlugin {
    config: Config,
}

impl AlumetPlugin for ProcessToCgroupBridgePlugin {
    fn name() -> &'static str {
        "process-to-cgroup-bridge"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(ProcessToCgroupBridgePlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let processes_metrics = self.config.processes_metrics.clone();
        let merge_similar_cgroups = self.config.merge_similar_cgroups;
        let keep_processed_measurements = self.config.keep_processed_measurements;

        #[cfg(test)]
        let proc_path = self.config.proc_path.clone();

        alumet.add_transform_builder("transform", move |ctx| {
            let mut processes_metrics_ids: Vec<RawMetricId> = Vec::new();
            for metric_name in &processes_metrics {
                processes_metrics_ids.push(
                    ctx.metric_by_name(metric_name)
                        .with_context(|| format!("Metric not found : {}", metric_name))?
                        .0,
                );
            }
            if processes_metrics.is_empty() {
                return Err(anyhow::anyhow!("no processes metrics was found either because `processes_metrics` config is empty or because no Alumet metric have matched the names"))
            }
            let transform = Box::new(ProcessToCgroupBridgeTransform::new(
                processes_metrics_ids,
                merge_similar_cgroups,
                keep_processed_measurements,

                #[cfg(test)]
                proc_path,
            ));
            Ok(transform)
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
pub struct Config {
    /// The metrics names we want to find the cgroup for
    pub processes_metrics: Vec<String>,
    /// Will aggregate measurements in case multiple processes share the same cgroup and have the same timestamp. This leads to one measurement per metric per cgroup per timestamp.
    pub merge_similar_cgroups: bool,
    /// Will keep all the measurements that have been processed by the transformer. In case it's false only the measurements with a cgroup resource consumer will be kept.
    pub keep_processed_measurements: bool,

    #[cfg(test)]
    pub proc_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            processes_metrics: vec![
                String::from("some_metric_to_bridge"),
                String::from("another_metric_to_bridge"),
            ],
            merge_similar_cgroups: true,
            keep_processed_measurements: true,

            #[cfg(test)]
            proc_path: PathBuf::from("/proc"),
        }
    }
}
