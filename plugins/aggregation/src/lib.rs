mod aggregations;
mod transform;

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use alumet::{
    metrics::{duplicate::DuplicateReaction, online::MetricSender, Metric, RawMetricId},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,

    metrics_list: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
}

impl AlumetPlugin for AggregationPlugin {
    fn name() -> &'static str {
        "aggregation"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
            metrics_list: Vec::<Metric>::new(),
            old_ids: Vec::<RawMetricId>::new(),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(AggregationTransform::new(
            self.config.interval,
            self.config.function,
            self.metric_correspondence_table.clone(),
        ));
        alumet.add_transform("plugin-aggregation", transform)?;

        // TODO: give metric sender to the transformPlugin P2

        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        let metrics = alumet.metrics();

        for metric_name in self.config.metrics.iter() {
            let (raw_metric_id, metric) = metrics
                .by_name(metric_name)
                .with_context(|| format!("metric \"{}\" not found", &metric_name))?;
            self.old_ids.push(raw_metric_id);
            let new_metric = Metric {
                name: format!("{metric_name}_{}", self.config.function.name()),
                unit: metric.unit.clone(),
                description: metric.description.clone(),
                value_type: metric.value_type.clone(),
            };

            self.metrics_list.push(new_metric);
        }

        if self.metrics_list.len() != self.old_ids.len() {
            return Err(anyhow!(
                "could not pre register one aggregated metric for each requested metrics"
            ));
        }

        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        alumet.metrics_sender();

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
            &mut alumet.metrics_sender(),
            self.metrics_list.clone(),
            self.old_ids.clone(),
            self.metric_correspondence_table.clone(),
        ))
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn register_new_metrics(
    metric_sender: &mut MetricSender,
    new_metrics: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
) -> anyhow::Result<()> {
    let result = metric_sender
        .create_metrics(new_metrics, DuplicateReaction::Error)
        .await
        .map_err(|a| anyhow!("{a}"))?;

    for (before, after) in std::iter::zip(old_ids, result) {
        let new_id = after?;

        let metric_correspondence_table_clone = &metric_correspondence_table.clone();
        let mut metric_correspondence_table_write = metric_correspondence_table_clone
            .write()
            .expect("metric_correspondence_table lock poisoned");

        metric_correspondence_table_write.insert(before, new_id);
    }
    Ok(())
}

#[derive(Deserialize, Serialize, Clone)]
struct Config {
    /// Interval for the aggregation.
    #[serde(with = "humantime_serde")]
    interval: Duration,

    // TODO: add boolean about moving aggregation window. P3

    // TODO: add boolean to drop or not the received metric point. P2

    // TODO: add possibility to choose if the generated timestamp is at the left, center or right of the interval. P3
    function: aggregations::Function,

    // List of metrics where to apply function.
    // Leave empty to apply function to every metrics. NO
    // TODO: manage all/* metrics P3
    metrics: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            function: aggregations::Function::Sum,
            metrics: Vec::<String>::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use alumet::plugin::rust::AlumetPlugin;

    use crate::AggregationPlugin;

    #[test]
    fn test_name() {
        assert_eq!(AggregationPlugin::name(), "aggregation");
    }

    #[test]
    fn test_version() {
        assert_eq!(AggregationPlugin::version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_init() {
        let _ = AggregationPlugin::init(AggregationPlugin::default_config().unwrap().unwrap()).unwrap();
    }
}
