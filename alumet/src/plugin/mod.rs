use std::error::Error;
use std::fmt;

use crate::metrics::{MetricId, MeasurementType};
use crate::pipeline;
use crate::pipeline::registry::{MetricRegistry, ElementRegistry};
use crate::units::Unit;
use crate::config::ConfigTable;
use crate::error::GenericError;

// pub mod dyn_load;

pub struct PluginInfo {
    pub name: String,
    pub version: String,
    // todo try to avoid boxing here?
    pub init: Box<dyn FnOnce(&mut ConfigTable) -> Result<Box<dyn Plugin>, PluginError>>,
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
    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), PluginError>;

    /// Stops the plugin.
    ///
    /// This method is called _after_ all the metrics, sources and outputs previously registered
    /// by [`Plugin::start`] have been stopped and unregistered.
    fn stop(&mut self) -> Result<(), PluginError>;
}

/// `AlumetStart` allows the plugins to perform some actions before starting the measurment pipeline,
/// such as registering new measurement sources.
pub struct AlumetStart<'a> {
    pub metrics: &'a mut MetricRegistry,
    pub pipeline_elements: &'a mut ElementRegistry
}

impl AlumetStart<'_> {
    pub fn create_metric(&mut self, name: &str, value_type: MeasurementType, unit: Unit, description: &str) -> Result<MetricId, PluginError> {
        self.metrics.create_metric(name,value_type,unit,description).map_err(|e| todo!(""))
    }
    pub fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        self.pipeline_elements.add_source(source)
    }
    pub fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        self.pipeline_elements.add_transform(transform)
    }
    pub fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        self.pipeline_elements.add_output(output)
    }
}

// ====== Errors ======

#[derive(Debug)]
pub struct PluginError(GenericError<PluginErrorKind>);

impl PluginError {
    pub fn new(kind: PluginErrorKind) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: None,
            description: None,
        })
    }

    pub fn with_description(kind: PluginErrorKind, description: &str) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: None,
            description: Some(description.to_owned()),
        })
    }

    pub fn with_cause<E: Error + 'static>(kind: PluginErrorKind, description: &str, cause: E) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: Some(Box::new(cause)),
            description: Some(description.to_owned()),
        })
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PluginErrorKind {
    /// The plugin's configuration could not be parsed or contains invalid entries.
    InvalidConfiguration,
    /// The plugin requires a sensor that could not be found.
    /// For example, the plugin fetches information from an internal wattmeter, but the host does not have one.
    SensorNotFound,
    /// The plugin attempted an IO operation, but failed.
    IoFailure,
}

impl fmt::Display for PluginErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginErrorKind::InvalidConfiguration => todo!(),
            PluginErrorKind::SensorNotFound => todo!(),
            PluginErrorKind::IoFailure => todo!(),
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
