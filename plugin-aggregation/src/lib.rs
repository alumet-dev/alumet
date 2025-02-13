mod transform;
mod aggregations;
mod transform;
mod transform;

use std::{
    collections::HashMap,
    rc::Rc,
    sync::{Arc, RwLock},
    time::Duration,
};
use std::{collections::HashMap, rc::Rc, sync::{Arc, RwLock}, time::Duration};
use std::{
    collections::HashMap,
    rc::Rc,
    sync::{Arc, RwLock},
    time::Duration,
};

use alumet::{
    metrics_list: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
    pipeline::registry::MetricSender,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
use alumet::{metrics::{Metric, RawMetricId}, pipeline::registry::MetricSender, plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    ConfigTable,
use alumet::{
    metrics::{Metric, RawMetricId},
    pipeline::registry::MetricSender,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};
}};
    },
        Ok(Box::new(AggregationPlugin {

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
}

    metrics_list: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
        Ok(Box::new(AggregationPlugin {

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
        Ok(Box::new(AggregationPlugin {

use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
            metrics_list: Vec::<Metric>::new(),
            old_ids: Vec::<RawMetricId>::new(),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
            metrics_list: Vec::<Metric>::new(),
            old_ids: Vec::<RawMetricId>::new(),
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
        let transform = Box::new(AggregationTransform::new(
            self.config.interval,
            self.config.function,
            self.metric_correspondence_table.clone(),
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(AggregationTransform::new(
            self.config.interval,
            self.config.function,
            self.metric_correspondence_table.clone(),
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(
            AggregationTransform::new(
                self.config.interval,
                self.config.function,
                self.metric_correspondence_table.clone(),
        ));
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let transform = Box::new(
            AggregationTransform::new(
                self.config.interval,
                self.config.function,
                self.metric_correspondence_table.clone(),
        ));
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
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
use serde::{Deserialize, Serialize};
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
        let mut metric_sender_ref = Rc::clone(&self.metric_sender);

        alumet.on_pipeline_start( move |ctx| {
            *Rc::get_mut(&mut metric_sender_ref).unwrap() = Some(ctx.metrics_sender());
            Ok(())
        });
pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
        let mut metric_sender_ref = Rc::clone(&self.metric_sender);

        alumet.on_pipeline_start( move |ctx| {
            *Rc::get_mut(&mut metric_sender_ref).unwrap() = Some(ctx.metrics_sender());
            Ok(())
        });

        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPreStart) -> anyhow::Result<()> {
        let metrics = alumet.metrics();

    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

            let (raw_metric_id, metric) = metrics.by_name(&metric_name).with_context(|| "metric not found")?;
            self.old_ids.push(raw_metric_id);
            let new_metric = Metric {
                name: format!("{metric_name}_{}", self.config.function.get_string()),
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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
            let (raw_metric_id, metric) = metrics.by_name(&metric_name).with_context(|| "metric not found")?;
            self.old_ids.push(raw_metric_id);
            let new_metric = Metric {
                name: format!("{metric_name}_{}", self.config.function.get_string()),
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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
                value_type: metric.value_type.clone(),
            };
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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
                value_type: metric.value_type.clone(),
            };

            self.metrics_list.push(new_metric);
        }
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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
            self.metrics_list.push(new_metric);
    }

        if self.metrics_list.len() != self.old_ids.len() {
            return Err(anyhow!("Pas normal"));
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin { 
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        if self.metrics_list.len() != self.old_ids.len() {
            return Err(anyhow!("Pas normal"));
        }

        Ok(())
    }
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin { 
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        alumet.metrics_sender();

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin { 
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

    fn post_pipeline_start(&mut self, alumet: &mut alumet::plugin::AlumetPostStart) -> anyhow::Result<()> {
        alumet.metrics_sender();


    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin { 
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

            &mut alumet.metrics_sender(),
            self.metrics_list.clone(),
            self.old_ids.clone(),
        Ok(Box::new(AggregationPlugin { 
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
            &mut alumet.metrics_sender(),
            self.metrics_list.clone(),
            self.old_ids.clone(),
use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
            Rc::get_mut(&mut self.metric_sender).unwrap().as_mut(),
            new_metrics,
            old_ids,

use transform::AggregationTransform;

pub struct AggregationPlugin {
    config: Config,
    metric_sender: Rc<Option<MetricSender>>,

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
            Rc::get_mut(&mut self.metric_sender).unwrap().as_mut(),
            new_metrics,
            old_ids,
            self.metric_correspondence_table.clone(),
        ));

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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
            Rc::get_mut(&mut self.metric_sender).unwrap().as_mut(),
            new_metrics,
            old_ids,
            self.metric_correspondence_table.clone(),
        ));
    metric_sender: &mut MetricSender,
    new_metrics: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    metric_sender: &mut MetricSender,
    new_metrics: Vec<Metric>,
    old_ids: Vec<RawMetricId>,
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
) {
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
) {
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    let result = metric_sender
        .create_metrics(new_metrics, alumet::pipeline::registry::DuplicateStrategy::Error)
        .await
        .unwrap();
    for (before, after) in std::iter::zip(old_ids, result) {
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
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    let result = metric_sender
        .create_metrics(new_metrics, alumet::pipeline::registry::DuplicateStrategy::Error)
        .await
        .unwrap();
    for (before, after) in std::iter::zip(old_ids, result) {

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
        let mut metric_correspondence_table_write = (*metric_correspondence_table_clone).write().unwrap();
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
        let mut metric_correspondence_table_write = (*metric_correspondence_table_clone).write().unwrap();
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
        metric_correspondence_table_write.insert(before, new_id);
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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

        metric_correspondence_table_write.insert(before, new_id);

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(AggregationPlugin {
            config,
            metric_sender: Rc::new(None),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
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

        // TODO: give metric sender to the transformPlugin P2
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

        // Let's create a runtime to await async function and fill hashmap
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(register_new_metrics(
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
