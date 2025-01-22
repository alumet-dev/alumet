mod transform;

use std::{rc::Rc, sync::Arc, time::Duration};

use alumet::{metrics::{Metric, RawMetricId}, pipeline::{elements::transform::builder::TransformRegistration, registry::MetricSender, trigger::BoxFuture}, plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    ConfigTable,
}};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use futures::executor;
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,
    // TODO: add correspondence table ? 
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
        Ok(Box::new(AggregationPlugin { config , metric_sender: Rc::new(None)}))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // TODO: Init the correspondence table
        let transform = Box::new(AggregationTransform::new(self.config.interval));
        alumet.add_transform(transform);

        // TODO:  give metric sender to the transformPlugin
        let mut metric_sender_ref = Rc::clone(&self.metric_sender);

        alumet.on_pipeline_start( move |ctx| {
            *Rc::get_mut(&mut metric_sender_ref).unwrap() = Some(ctx.metrics_sender());
            Ok(())
        });

        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        let metrics = alumet.metrics();

        let mut new_metrics = Vec::<Metric>::new();
        let mut old_ids = Vec::<RawMetricId>::new();

        for metric_name in self.config.metrics.iter() {
            let (raw_metric_id, metric) = metrics
                .by_name(&metric_name)
                .with_context(|| "metric not found")?;
            old_ids.push(raw_metric_id);
            let new_metric = Metric{
                name: format!("{metric_name}-{}", self.config.function),
                unit: metric.unit.clone(),
                description: metric.description.clone(),
                value_type: metric.value_type.clone()};

            new_metrics.push(new_metric);
        }
        // TODO: add verif on len of old_ids and new mtridcs
        // TODO: add newRawMetricId to correspondence_table
        let handle = Handle::current();
        let _ = handle.enter();
        futures::executor::block_on(truc(
            Rc::get_mut(&mut self.metric_sender).unwrap().as_mut(),
            new_metrics,
            old_ids,
        ));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn truc(test: Option<&mut MetricSender>, new_metrics:Vec<Metric>, old_ids: Vec<RawMetricId>) {
    let reuslt = test.unwrap().create_metrics(new_metrics, alumet::pipeline::registry::DuplicateStrategy::Error).await.unwrap();
    for (before, after) in std::iter::zip(old_ids, reuslt) {
        let new_id = after.unwrap();

        
    }
} 

#[derive(Deserialize, Serialize, Clone)]
struct Config {
    /// Interval for the aggregation.
    #[serde(with = "humantime_serde")]
    interval: Duration,

    // TODO: add boolean about moving aggregation window.

    // TODO: add boolean to drop or not the received metric point

    // TODO: move from string to enum with sum, mean, etc.
    function: String,

    // List of metrics where to apply function.
    // Leave empty to apply function to every metrics. NO
    // TODO: manage all/* metrics
    metrics: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(60),
            function: "sum".to_string(),
            metrics: Vec::<String>::new(),
        }
    }
}
