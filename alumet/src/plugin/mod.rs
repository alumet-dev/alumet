use crate::config::ConfigTable;
use crate::metrics::{WrappedMeasurementType, MetricId, MeasurementType, TypedMetricId, UntypedMetricId};
use crate::pipeline;
use crate::pipeline::registry::{ElementRegistry, MetricCreationError, MetricRegistry};
use crate::units::Unit;

#[cfg(feature = "dynamic")]
mod dyn_ffi;
#[cfg(feature = "dynamic")]
pub mod dyn_load;

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

    pub fn create_metric<T: MeasurementType>(
        &mut self,
        name: &str,
        unit: Unit,
        description: &str,
    ) -> Result<TypedMetricId<T>, MetricCreationError> {
        let untyped = self.metrics.create_metric(name, T::wrapped_type(), unit, description)?;
        Ok(TypedMetricId::try_from(untyped, &self.metrics).unwrap())
    }
    
    pub fn create_metric_untyped(
        &mut self,
        name: &str,
        value_type: WrappedMeasurementType,
        unit: Unit,
        description: &str,
    ) -> Result<UntypedMetricId, MetricCreationError> {
        self.metrics.create_metric(name, value_type, unit, description)
    }

    pub fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_source(plugin, source)
    }
    pub fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_transform(plugin, transform)
    }

    pub fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        let plugin = self.get_current_plugin_name();
        self.pipeline_elements.add_output(plugin, output)
    }
}
