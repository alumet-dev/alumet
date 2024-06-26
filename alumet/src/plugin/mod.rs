//! Static and dynamic plugins.
//!
//! Plugins are an essential part of Alumet, as they provide the
//! [`Source`]s, [`Transform`]s and [`Output`]s.
//!
//! ## Plugin lifecycle
//! Every plugin follow these steps:
//!
//! 1. **Metadata loading**: the Alumet application loads basic information about the plugin.
//! For static plugins, this is done at compile time (cargo takes care of the dependencies).
//!
//! 2. **Initialization**: the plugin is initialized, a value that implements the [`Plugin`] trait is created.
//! During the initialization phase, the plugin can read its configuration.
//!
//! 3. **Start-up**: the plugin is started via its [`start`](Plugin::start) method.
//! During the start-up phase, the plugin can create metrics, register new pipeline elements
//! (sources, transforms, outputs), and more.
//!
//! 4. **Operation**: the measurement pipeline has started, and the elements registered by the plugin
//! are in use. Alumet takes care of the lifetimes and of the triggering of those elements.
//!
//! 5. **Stop**: the elements registered by the plugin are stopped and dropped.
//! Then, [`stop`](Plugin::stop) is called.
//!
//! 6. **Drop**: like any Rust value, the plugin is dropped when it goes out of scope.
//! To customize the destructor of your static plugin, implement the [`Drop`] trait on your plugin structure.
//! For a dynamic plugin, modify the `plugin_drop` function.
//!
//! ## Static plugins
//!
//! A static plugin is a plugin that is included in the Alumet measurement application
//! at compile-time. The measurement tool and the static plugin are compiled together,
//! in a single executable binary.
//!
//! To create a static plugin in Rust, make a new library crate that depends on:
//! - the core of Alumet (the `alumet` crate).
//! - the `anyhow` crate
//!
//! To do this, here is an example (bash commands):
//! ```sh
//! cargo init --lib my-plugin
//! cd my-plugin
//! cargo add alumet anyhow
//! ```
//!
//! In your library, define a structure for your plugin, and implement the [`rust::AlumetPlugin`] trait for it.
//! ```no_run
//! use alumet::plugin::{rust::AlumetPlugin, AlumetStart, ConfigTable};
//!
//! struct MyPlugin {}
//!
//! impl AlumetPlugin for MyPlugin {
//!     fn name() -> &'static str {
//!         "my-plugin"
//!     }
//!
//!     fn version() -> &'static str {
//!         "0.1.0"
//!     }
//!
//!     fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
//!         // You can read the config and store some settings in your structure.
//!         Ok(Box::new(MyPlugin {}))
//!     }
//!
//!     fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()> {
//!         println!("My first plugin is starting!");
//!         Ok(())
//!     }
//!
//!     fn stop(&mut self) -> anyhow::Result<()> {
//!         println!("My first plugin is stopping!");
//!         Ok(())
//!     }
//! }
//! ```
//!
//! Finally, modify the measurement application in the following ways:
//! 1. Add a dependency to your plugin crate (for example with `cargo add my-plugin --path=path/to/my-plugin`).
//! 2. Modify your `main` to initialize and load the plugin. See [`Agent`](crate::agent::Agent).
//!
//! ## Dynamic plugins
//!
//! WIP
//!
use std::future::Future;
use std::marker::PhantomData;

use tokio_util::sync::CancellationToken;

use crate::measurement::{MeasurementBuffer, MeasurementType, WrappedMeasurementType};
use crate::metrics::{Metric, MetricCreationError, RawMetricId, TypedMetricId};
use crate::pipeline::builder::{AutonomousSourceBuilder, ManagedSourceBuilder, OutputBuilder, TransformBuilder};
use crate::pipeline::runtime::{IdlePipeline, RunningPipeline};
use crate::pipeline::trigger::TriggerSpec;
use crate::pipeline::{builder::PendingPipelineContext, builder::PipelineBuilder};
use crate::pipeline::{Output, Source, Transform};
use crate::units::PrefixedUnit;

use self::rust::AlumetPlugin;

#[cfg(feature = "dynamic")]
pub mod dynload;

pub mod event;
pub mod rust;
pub mod util;
pub(crate) mod version;

/// Plugin metadata, and a function that allows to initialize the plugin.
pub struct PluginMetadata {
    /// Name of the plugin, must be unique.
    pub name: String,
    /// Version of the plugin, should follow semantic versioning (of the form `x.y.z`).
    pub version: String,
    /// Function that initializes the plugin.
    pub init: Box<dyn FnOnce(ConfigTable) -> anyhow::Result<Box<dyn Plugin>>>,
    /// Function that returns a default configuration for the plugin, or None
    /// if the plugin has no configurable option.
    ///
    /// The default config is used to generate the configuration file of the
    /// Alumet agent, in case it does not exist. In other cases, the default
    /// config returned by this function is not used, including when
    pub default_config: Box<dyn Fn() -> anyhow::Result<Option<ConfigTable>>>,
}

impl PluginMetadata {
    /// Build a metadata structure for a static plugin that implements [`AlumetPlugin`].
    pub fn from_static<P: AlumetPlugin + 'static>() -> Self {
        Self {
            name: P::name().to_owned(),
            version: P::version().to_owned(),
            init: Box::new(|conf| P::init(conf).map(|p| p as _)),
            default_config: Box::new(P::default_config),
        }
    }
}

/// A configuration table for plugins.
///
/// `ConfigTable` is currently a wrapper around [`toml::Table`].
/// However, you probably don't need to add a dependency on the `toml` crate,
/// since Alumet provides functions to easily serialize and deserialize configurations
/// with `serde`.
///
/// ## Example
///
/// ```
/// use serde::{Serialize, Deserialize};
/// use alumet::plugin::ConfigTable;
/// use alumet::plugin::rust::{serialize_config, deserialize_config};
///
/// #[derive(Serialize, Deserialize)]
/// struct MyConfig {
///     field: String
/// }
///
/// // serialize struct to config
/// let my_struct = MyConfig { field: String::from("value") };
/// let serialized: ConfigTable = serialize_config(my_struct).expect("serialization failed");
///
/// // deserialize config to struct
/// let my_table: ConfigTable = serialized;
/// let deserialized: MyConfig = deserialize_config(my_table).expect("deserialization failed");
/// ```
#[derive(Debug, Clone)]
pub struct ConfigTable(pub toml::Table);

/// Trait for plugins.
///
/// ## Note for plugin authors
///
/// You should _not_ implement this trait manually.
///
/// If you are writing a plugin in Rust, implement [`AlumetPlugin`] instead.
/// If you are writing a plugin in C, you need to define the right symbols in your shared library.
pub trait Plugin {
    /// The name of the plugin. It must be unique: two plugins cannot have the same name.
    fn name(&self) -> &str;

    /// The version of the plugin, for instance `"1.2.3"`. It should adhere to semantic versioning.
    fn version(&self) -> &str;

    /// Starts the plugin, allowing it to register metrics, sources and outputs.
    ///
    /// ## Plugin restart
    /// A plugin can be started and stopped multiple times, for instance when ALUMET switches from monitoring to profiling mode.
    /// [`Plugin::stop`] is guaranteed to be called between two calls of [`Plugin::start`].
    fn start(&mut self, alumet: &mut AlumetStart) -> anyhow::Result<()>;

    /// Stops the plugin.
    ///
    /// This method is called _after_ all the metrics, sources and outputs previously registered
    /// by [`Plugin::start`] have been stopped and unregistered.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Function called between the plugin startup phase and the operation phase.
    ///
    /// It can be used, for instance, to examine the metrics that have been registered.
    /// No modification to the pipeline can be applied.
    fn pre_pipeline_start(&mut self, pipeline: &IdlePipeline) -> anyhow::Result<()>;

    /// Function called after the beginning of the operation phase,
    /// i.e. the measurement pipeline has started.
    ///
    /// It can be used, for instance, to obtain a [`ControlHandle`](crate::pipeline::runtime::ControlHandle)
    /// of the pipeline.
    fn post_pipeline_start(&mut self, pipeline: &mut RunningPipeline) -> anyhow::Result<()>;
}

/// Structure passed to plugins for the start-up phase.
///
/// It allows the plugins to perform some actions before starting the measurment pipeline,
/// such as registering new measurement sources.
///
/// ## Note for applications
/// You should not create `AlumetStart` manually, build an [`Agent`](crate::agent::Agent) instead.
pub struct AlumetStart<'a> {
    pub(crate) pipeline_builder: &'a mut PipelineBuilder,
    pub(crate) current_plugin_name: String,
}

impl<'a> AlumetStart<'a> {
    /// Returns the name of the plugin that is being started.
    fn current_plugin_name(&self) -> &str {
        &self.current_plugin_name
    }

    pub fn new(pipeline_builder: &'a mut PipelineBuilder, plugin_name: String) -> AlumetStart<'a> {
        Self {
            pipeline_builder,
            current_plugin_name: plugin_name,
        }
    }

    /// Creates a new metric with a measurement type `T` (checked at compile time).
    /// Fails if a metric with the same name already exists.
    pub fn create_metric<T: MeasurementType>(
        &mut self,
        name: impl Into<String>,
        unit: impl Into<PrefixedUnit>,
        description: impl Into<String>,
    ) -> Result<TypedMetricId<T>, MetricCreationError> {
        let m = Metric {
            name: name.into(),
            description: description.into(),
            value_type: T::wrapped_type(),
            unit: unit.into(),
        };
        let untyped_id = self.pipeline_builder.metrics.register(m)?;
        Ok(TypedMetricId(untyped_id, PhantomData))
    }

    /// Creates a new metric with a measurement type `value_type` (checked at **run time**).
    /// Fails if a metric with the same name already exists.
    ///
    /// Unlike [`TypedMetricId`], an [`RawMetricId`] does not allow to check that the
    /// measured values are of the right type at compile time.
    /// It is better to use [`create_metric`](Self::create_metric).
    pub fn create_metric_untyped(
        &mut self,
        name: &str,
        value_type: WrappedMeasurementType,
        unit: impl Into<PrefixedUnit>,
        description: &str,
    ) -> Result<RawMetricId, MetricCreationError> {
        let m = Metric {
            name: name.to_owned(),
            description: description.to_owned(),
            value_type,
            unit: unit.into(),
        };
        self.pipeline_builder.metrics.register(m)
    }

    /// Adds a measurement source to the Alumet pipeline.
    pub fn add_source(&mut self, source: Box<dyn Source>, trigger: TriggerSpec) {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/source"), true);
        self.pipeline_builder.sources.push(ManagedSourceBuilder {
            name,
            plugin,
            trigger,
            build: Box::new(|_| source),
        })
    }

    /// Adds the builder of a measurement source to the Alumet pipeline.
    ///
    /// Unlike [`add_source`](Self::add_source), the source is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the source. A good use case is to access the late registration of metrics.
    ///
    /// The downside is a more complicated code.
    /// In general, you should prefer to use [`add_source`](Self::add_source) if possible.
    pub fn add_source_builder<F: FnOnce(&PendingPipelineContext) -> Box<dyn Source> + 'static>(
        &mut self,
        trigger: TriggerSpec,
        source_builder: F,
    ) {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/source"), true);
        self.pipeline_builder.sources.push(ManagedSourceBuilder {
            name,
            plugin,
            trigger,
            build: Box::new(source_builder),
        });
    }

    /// Adds the builder of an autonomous source to the Alumet pipeline.
    ///
    /// An autonomous source is not triggered by Alumet, but runs independently.
    /// It is given a [`Sender`](tokio::sync::mpsc::Sender) to send its measurements
    /// to the rest of the Alumet pipeline (transforms and outputs).
    /// 
    /// ## Graceful shutdown
    /// To stop the autonomous source, a [`CancellationToken`] is provided.
    /// When the token is cancelled, you should stop the source.
    ///
    /// ## Example
    /// ```no_run
    /// use std::time::SystemTime;
    /// use alumet::measurement::{MeasurementBuffer, MeasurementPoint, Timestamp};
    /// use alumet::units::Unit;
    /// # use alumet::plugin::AlumetStart;
    ///
    /// # let alumet: &AlumetStart = todo!();
    /// let metric = alumet.create_metric::<u64>("my_metric", Unit::Second, "...").unwrap();
    /// alumet.add_autonomous_source(move |_, cancel_token, tx| {
    ///     let out_tx = tx.clone();
    ///     async move {
    ///         let mut buf = MeasurementBuffer::new();
    ///         while !cancel_token.is_cancelled() {
    ///             let timestamp = Timestamp::now();
    ///             let resource = todo!();
    ///             let consumer = todo!();
    ///             let value = todo!();
    ///             let measurement = MeasurementPoint::new(
    ///                 timestamp,
    ///                 metric,
    ///                 resource,
    ///                 consumer,
    ///                 value
    ///             );
    ///             buf.push(measurement);
    ///             out_tx.send(buf.clone());
    ///             buf.clear();
    ///         }
    ///         Ok(())
    ///     }
    /// })
    /// ```
    pub fn add_autonomous_source<F, S>(&mut self, source_builder: F)
    where
        F: FnOnce(&PendingPipelineContext, CancellationToken, tokio::sync::mpsc::Sender<MeasurementBuffer>) -> S + 'static,
        S: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/autonomous_source"), true);
        self.pipeline_builder.autonomous_sources.push(AutonomousSourceBuilder {
            name,
            plugin,
            build: Box::new(|p: &_, cancel, tx| Box::pin(source_builder(p, cancel, tx))),
        })
    }

    /// Adds a transform step to the Alumet pipeline.
    pub fn add_transform(&mut self, transform: Box<dyn Transform>) {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/transform"), true);
        self.pipeline_builder.transforms.push(TransformBuilder {
            name,
            plugin,
            build: Box::new(|_| transform),
        });
    }

    /// Adds an output to the Alumet pipeline.
    pub fn add_output(&mut self, output: Box<dyn Output>) {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/output"), true);
        self.pipeline_builder.outputs.push(OutputBuilder {
            name,
            plugin,
            build: Box::new(|_| Ok(output)),
        })
    }

    /// Adds the builder of an output to the Alumet pipeline.
    ///
    /// Unlike [`add_output`](Self::add_output), the output is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the output. A good use case is to access the tokio runtime [`Handle`](tokio::runtime::Handle)
    /// in order to use an async library.
    ///
    /// In general, you should prefer to use [`add_output`](Self::add_output) if possible.
    pub fn add_output_builder<F: FnOnce(&PendingPipelineContext) -> anyhow::Result<Box<dyn Output>> + 'static>(
        &mut self,
        output_builder: F,
    ) {
        let plugin = self.current_plugin_name().to_owned();
        let name = self
            .pipeline_builder
            .namegen
            .deduplicate(format!("{plugin}/output"), true);
        self.pipeline_builder.outputs.push(OutputBuilder {
            name,
            plugin,
            build: Box::new(output_builder),
        })
    }
}
