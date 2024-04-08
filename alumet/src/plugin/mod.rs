use std::marker::PhantomData;

use crate::config::ConfigTable;
use crate::measurement::{MeasurementType, WrappedMeasurementType};
use crate::metrics::{Metric, MetricCreationError, MetricRegistry, RawMetricId, TypedMetricId};
use crate::pipeline;
use crate::pipeline::registry::ElementRegistry;
use crate::units::Unit;

#[cfg(feature = "dynamic")]
pub mod dynload;

// Module to handle versioning.
pub(crate) mod version;

pub struct PluginInfo {
    pub name: String,
    pub version: String,
    // todo try to avoid boxing here?
    pub init: Box<dyn FnOnce(&mut ConfigTable) -> anyhow::Result<Box<dyn Plugin>>>,
}

/// The ALUMET plugin trait.
///
/// Plugins are a central part of ALUMET, because they produce, transform and export the measurements.
/// Please refer to the module documentation.
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
}

/// Provides [`AlumetStart`] to start plugins.
pub struct PluginStarter<'a> {
    start: AlumetStart<'a>,
}

impl<'a> PluginStarter<'a> {
    pub fn new(metrics: &'a mut MetricRegistry, pipeline_elements: &'a mut ElementRegistry) -> Self {
        PluginStarter {
            start: AlumetStart {
                metrics,
                pipeline_elements,
                current_plugin_name: None,
            },
        }
    }

    pub fn start(&mut self, plugin: &mut dyn Plugin) -> anyhow::Result<()> {
        self.start.current_plugin_name = Some(plugin.name().to_owned());
        plugin.start(&mut self.start)
    }
}

/// `AlumetStart` allows the plugins to perform some actions before starting the measurment pipeline,
/// such as registering new measurement sources.
pub struct AlumetStart<'a> {
    metrics: &'a mut MetricRegistry,
    pipeline_elements: &'a mut ElementRegistry,
    current_plugin_name: Option<String>,
}

impl AlumetStart<'_> {
    fn get_current_plugin_name(&self) -> String {
        self.current_plugin_name
            .clone()
            .expect("The current plugin must be set before passing the AlumetStart struct to a plugin")
    }

    /// Creates a new metric with a measurement type `T` (checked at compile time).
    /// Fails if a metric with the same name already exists.
    pub fn create_metric<T: MeasurementType>(
        &mut self,
        name: &str,
        unit: Unit,
        description: &str,
    ) -> Result<TypedMetricId<T>, MetricCreationError> {
        let m = Metric {
            id: RawMetricId(usize::MAX),
            name: name.to_owned(),
            description: description.to_owned(),
            value_type: T::wrapped_type(),
            unit,
        };
        let untyped_id = self.metrics.register(m)?;
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
        unit: Unit,
        description: &str,
    ) -> Result<RawMetricId, MetricCreationError> {
        let m = Metric {
            id: RawMetricId(usize::MAX),
            name: name.to_owned(),
            description: description.to_owned(),
            value_type,
            unit,
        };
        let id = self.metrics.register(m)?;
        Ok(id.clone())
    }

    /// Adds a measurement source to the Alumet pipeline.
    pub fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_source(plugin, source)
    }
    
    /// Adds a transform step to the Alumet pipeline.
    pub fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_transform(plugin, transform)
    }

    /// Adds an output to the Alumet pipeline.
    pub fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_output(plugin, output)
    }
}
