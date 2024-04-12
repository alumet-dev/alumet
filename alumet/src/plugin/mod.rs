//! Static and dynamic plugins.
//!
//! Plugins are an essential part of Alumet, as they provide the
//! [`Source`](super::pipeline::Source)s, [`Transform`](super::pipeline::Transform)s and [`Output`](super::pipeline::Output)s.
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
//! In your library, define a structure for your plugin, and implement the [`Plugin`] trait for it.
//! ```no_run
//! use alumet::plugin::{Plugin, AlumetStart};
//!
//! struct MyPlugin {}
//!
//! impl Plugin for MyPlugin {
//!     fn name(&self) -> &str {
//!         "my-plugin"
//!     }
//!
//!     fn version(&self) -> &str {
//!         "0.1.0"
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
//! 2. Modify your `main` to initialize and load the plugin.
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
use crate::units::{CustomUnit, CustomUnitId, CustomUnitRegistry, PrefixedUnit, UnitCreationError};

use self::rust::AlumetPlugin;

#[cfg(feature = "dynamic")]
pub mod dynload;

pub mod manage;
pub mod rust;
pub(crate) mod version;

/// Plugin metadata, and a function that allows to initialize the plugin.
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub init: Box<dyn FnOnce(&mut ConfigTable) -> anyhow::Result<Box<dyn Plugin>>>,
}

impl PluginMetadata {
    pub fn from_static<P: AlumetPlugin + 'static>() -> Self {
        Self {
            name: P::name().to_owned(),
            version: P::version().to_owned(),
            init: Box::new(|conf| P::init(conf).map(|p| p as _)),
        }
    }
}

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
}

/// Structure passed to plugins for the start-up phase.
///
/// It allows the plugins to perform some actions before starting the measurment pipeline,
/// such as registering new measurement sources.
///
/// Note for applications: an `AlumetStart` should not be directly created, use [`PluginStartup`] instead.
pub struct AlumetStart<'a> {
    metrics: &'a mut MetricRegistry,
    units: &'a mut CustomUnitRegistry,
    pipeline_elements: &'a mut ElementRegistry,
    current_plugin_name: String,
}

impl AlumetStart<'_> {
    /// Returns the name of the plugin that is being started.
    fn current_plugin_name(&self) -> &str {
        &self.current_plugin_name
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
            id: RawMetricId(usize::MAX),
            name: name.into(),
            description: description.into(),
            value_type: T::wrapped_type(),
            unit: unit.into(),
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
        unit: impl Into<PrefixedUnit>,
        description: &str,
    ) -> Result<RawMetricId, MetricCreationError> {
        let m = Metric {
            id: RawMetricId(usize::MAX),
            name: name.to_owned(),
            description: description.to_owned(),
            value_type,
            unit: unit.into(),
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
