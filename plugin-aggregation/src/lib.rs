mod transform;

use std::time::Duration;
use std::sync::Arc;

use alumet::{metrics::Metric, pipeline::{elements::transform::builder::TransformRegistration, registry::MetricSender, trigger::BoxFuture}, plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    ConfigTable,
}};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Arc<Option<MetricSender>>,
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
        Ok(Box::new(AggregationPlugin { config , metric_sender: Arc::new(None)}))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(AggregationTransform::new(self.config.interval));
        alumet.add_transform(transform);
        // let config_clone = self.config.clone();
        
        // alumet.add_transform_builder(
        //     move |ctx| {
        //         let name = ctx.transform_name("plugin-aggregation");

        //         {
        //             for metric_name in config_clone.metrics.iter() {
        //                 let (raw_metric_id, metric) = ctx
        //                     .metric_by_name(&metric_name)
        //                     .with_context(|| "metric not found")?;

        //                 // let new_metric: alumet::metrics::TypedMetricId<f64> = alumet.create_metric(format!("{metric_name}-{}", config_clone.function), metric.unit, metric.description)?;
                    
                        
        //             }
        //         }

        //         let transform = Box::new(AggregationTransform::new(config_clone.interval));
        //         Ok(TransformRegistration{name, transform})
        //     },
        // );

        let mut metric_sender_clone = Arc::get_mut(&mut self.metric_sender).unwrap();

        alumet.on_pipeline_start(move |ctx| {
            metric_sender_clone = &mut Some(ctx.metrics_sender());
            Ok(())
        });
        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        let metrics = alumet.metrics();

        let mut new_metrics = Vec::<Metric>::new();

        for metric_name in self.config.metrics.iter() {
            let (raw_metric_id, metric) = metrics
                .by_name(&metric_name)
                .with_context(|| "metric not found")?;

            let new_metric = Metric{
                name: format!("{metric_name}-{}", self.config.function),
                unit: metric.unit.clone(),
                description: metric.description.clone(),
                value_type: metric.value_type.clone()};

            new_metrics.push(new_metric);
        }

        Ok(())

        // self.metric_sender.create_metrics(new_metrics, alumet::pipeline::registry::DuplicateStrategy::Error)
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize, Clone)]
struct Config {
    /// Interval for the aggregation.
    #[serde(with = "humantime_serde")]
    interval: Duration,

    // TODO: add boolean about moving aggregation window.

    // TODO: move from string to enum with sum, mean, etc.
    function: String,

    // List of metrics where to apply function.
    // Leave empty to apply function to every metrics. NO
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
