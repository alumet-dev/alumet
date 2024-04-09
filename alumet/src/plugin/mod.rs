//! Static and dynamic plugins.
//!
//! Plugins are an essential part of Alumet, as they provide the
//! [`Source`](super::pipeline::Source)s, [`Transform`](super::pipeline::Transform)s and [`Output`](super::pipeline::Output)s.
//! 
//! ## Static plugins
//! 
//! 
//! ## Dynamic plugins
//! 
//! WIP
//! 
use std::marker::PhantomData;

use crate::config::ConfigTable;
use crate::measurement::{MeasurementType, WrappedMeasurementType};
use crate::metrics::{Metric, MetricCreationError, MetricRegistry, RawMetricId, TypedMetricId};
use crate::pipeline;
use crate::pipeline::registry::ElementRegistry;
use crate::units::{CustomUnit, CustomUnitId, CustomUnitRegistry, Unit, UnitCreationError};

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

pub struct PluginStartup {
    pub metrics: MetricRegistry,
    pub units: CustomUnitRegistry,
    pub pipeline_elements: ElementRegistry,
}

impl PluginStartup {
    pub fn new() -> Self {
        Self {
            metrics: MetricRegistry::new(),
            units: CustomUnitRegistry::new(),
            pipeline_elements: ElementRegistry::new(),
        }
    }

    pub fn start(&mut self, plugin: &mut dyn Plugin) -> anyhow::Result<()> {
        let mut start = AlumetStart {
            metrics: &mut self.metrics,
            units: &mut self.units,
            pipeline_elements: &mut self.pipeline_elements,
            current_plugin_name: plugin.name().to_owned(),
        };
        plugin.start(&mut start)
    }
}

/// `AlumetStart` allows the plugins to perform some actions before starting the measurment pipeline,
/// such as registering new measurement sources.
pub struct AlumetStart<'a> {
    metrics: &'a mut MetricRegistry,
    units: &'a mut CustomUnitRegistry,
    pipeline_elements: &'a mut ElementRegistry,
    current_plugin_name: String,
}

impl AlumetStart<'_> {
    fn current_plugin_name(&self) -> &str {
        &self.current_plugin_name
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
        self.metrics.register(m)
    }

    /// Creates a new unit of measurement.
    /// Fails if a unit with the same name already exists.
    ///
    /// To use the unit in measurement points, obtain its `CustomUnitId`
    /// and wrap it in [`Unit::Custom`].
    pub fn create_unit(&mut self, unit: CustomUnit) -> Result<CustomUnitId, UnitCreationError> {
        self.units.register(unit)
    }

    /// Adds a measurement source to the Alumet pipeline.
    pub fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        let plugin = self.current_plugin_name().to_owned();
        self.pipeline_elements.add_source(plugin, source)
    }

    /// Adds a transform step to the Alumet pipeline.
    pub fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        let plugin = self.current_plugin_name().to_owned();
        self.pipeline_elements.add_transform(plugin, transform)
    }

    /// Adds an output to the Alumet pipeline.
    pub fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        let plugin = self.current_plugin_name().to_owned();
        self.pipeline_elements.add_output(plugin, output)
    }
}
