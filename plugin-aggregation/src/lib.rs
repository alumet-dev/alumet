mod transform;
mod aggregations;

use std::{collections::HashMap, rc::Rc, sync::{Arc, RwLock}, time::Duration};

use alumet::{metrics::{Metric, RawMetricId, TypedMetricId}, pipeline::registry::MetricSender, plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    ConfigTable,
}};

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
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
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<u64, u64>::new())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(
            AggregationTransform::new(
                self.config.interval,
                self.config.function,
                self.metric_correspondence_table.clone(),
            )
        );
        alumet.add_transform(transform);

        // TODO:  give metric sender to the transformPlugin P2
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
                name: format!("{metric_name}-{}", self.config.function.get_string()),
                unit: metric.unit.clone(),
                description: metric.description.clone(),
                value_type: metric.value_type.clone()};

            new_metrics.push(new_metric);
        }

        if new_metrics.len() != old_ids.len() {
            return Err(anyhow!("Pas normal"))
        }

        let handle = Handle::current();
        let _ = handle.enter();
        futures::executor::block_on(register_new_metrics(
            Rc::get_mut(&mut self.metric_sender).unwrap().as_mut(),
            new_metrics,
            old_ids,
            self.metric_correspondence_table.clone(),
        ));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

async fn register_new_metrics(
        metric_sender: Option<&mut MetricSender>,
        new_metrics:Vec<Metric>,
        old_ids: Vec<RawMetricId>,
        metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
    ) {

    let reuslt = metric_sender.unwrap().create_metrics(new_metrics, alumet::pipeline::registry::DuplicateStrategy::Error).await.unwrap();
    for (before, after) in std::iter::zip(old_ids, reuslt) {
        let new_id = after.unwrap();
        let metric_correspondence_table_clone = Arc::clone(&metric_correspondence_table.clone());
        let mut bis = (*metric_correspondence_table_clone).write().unwrap();

        bis.insert(before.as_u64(), new_id.as_u64());
    }
} 

#[derive(Deserialize, Serialize, Clone)]
struct Config {
    /// Interval for the aggregation.
    #[serde(with = "humantime_serde")]
    interval: Duration,

    // TODO: add boolean about moving aggregation window. P3

    // TODO: add boolean to drop or not the received metric point P2

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
