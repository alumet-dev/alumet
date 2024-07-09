use std::marker::PhantomData;

use crate::pipeline::builder::context::OutputBuildContext;
use crate::pipeline::builder::elements::{
    AutonomousSourceBuilder, ManagedSourceBuilder, ManagedSourceRegistration, OutputBuilder, OutputRegistration,
    SourceBuilder, TransformBuilder, TransformRegistration,
};
use crate::pipeline::builder;
use crate::{
    measurement::{MeasurementType, WrappedMeasurementType},
    metrics::{Metric, MetricCreationError, RawMetricId, TypedMetricId},
    pipeline::{self, trigger, Output, PluginName, Source, Transform},
    units::PrefixedUnit,
};

/// Structure passed to plugins for the start-up phase.
///
/// It allows the plugins to perform some actions before starting the measurement pipeline,
/// such as registering new measurement sources.
///
/// ## Note for applications
/// You should not create `AlumetStart` manually, build an [`Agent`](crate::agent::Agent) instead.
pub struct AlumetStart<'a> {
    pub(crate) current_plugin: PluginName,
    pub(crate) pipeline_builder: &'a mut pipeline::Builder,
}

impl<'a> AlumetStart<'a> {
    /// Returns the name of the plugin that is being started.
    fn current_plugin_name(&self) -> PluginName {
        self.current_plugin.clone()
    }

    pub fn new(pipeline_builder: &'a mut pipeline::Builder, plugin: PluginName) -> AlumetStart<'a> {
        Self {
            pipeline_builder,
            current_plugin: plugin,
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
    pub fn add_source(&mut self, source: Box<dyn Source>, trigger: trigger::TriggerSpec) {
        let plugin = self.current_plugin_name();
        let builder = |ctx: &mut dyn builder::context::SourceBuildContext| ManagedSourceRegistration {
            name: ctx.source_name(""),
            trigger,
            source,
        };
        self.pipeline_builder
            .add_source_builder(plugin, SourceBuilder::Managed(Box::new(builder)))
    }

    /// Adds the builder of a measurement source to the Alumet pipeline.
    ///
    /// Unlike [`add_source`](Self::add_source), the source is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the source. A good use case is to access the late registration of metrics.
    ///
    /// The downside is a more complicated code.
    /// In general, you should prefer to use [`add_source`](Self::add_source) if possible.
    pub fn add_source_builder<F: ManagedSourceBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_source_builder(plugin, SourceBuilder::Managed(Box::new(builder)));
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
    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_source_builder(plugin, SourceBuilder::Autonomous(Box::new(builder)));
    }

    /// Adds a transform step to the Alumet pipeline.
    pub fn add_transform(&mut self, transform: Box<dyn Transform>) {
        let plugin = self.current_plugin_name();
        let builder = |ctx: &mut dyn builder::context::TransformBuildContext| TransformRegistration {
            name: ctx.transform_name(""),
            transform,
        };
        self.pipeline_builder.add_transform_builder(plugin, Box::new(builder));
    }

    /// todo doc
    pub fn add_transform_builder<F: TransformBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder.add_transform_builder(plugin, Box::new(builder));
    }

    /// Adds an output to the Alumet pipeline.
    pub fn add_output(&mut self, output: Box<dyn Output>) {
        let plugin = self.current_plugin_name();
        let builder = |ctx: &mut dyn OutputBuildContext| OutputRegistration {
            name: ctx.output_name(""),
            output,
        };
        self.pipeline_builder.add_output_builder(plugin, Box::new(builder));
    }

    /// Adds the builder of an output to the Alumet pipeline.
    ///
    /// Unlike [`add_output`](Self::add_output), the output is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the output. A good use case is to access the tokio runtime [`Handle`](tokio::runtime::Handle)
    /// in order to use an async library.
    ///
    /// In general, you should prefer to use [`add_output`](Self::add_output) if possible.
    pub fn add_output_builder<F: OutputBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder.add_output_builder(plugin, Box::new(builder));
    }
}

/// Structure passed to plugins for the post start-up phase.
pub struct AlumetPostStart<'a> {
    pub(crate) current_plugin: PluginName,
    pub(crate) pipeline: &'a mut pipeline::MeasurementPipeline,
}

impl<'a> AlumetPostStart<'a> {
    pub fn new(pipeline: &'a mut pipeline::MeasurementPipeline, plugin: PluginName) -> AlumetPostStart<'a> {
        Self {
            pipeline,
            current_plugin: plugin,
        }
    }
    
    
    /// Returns the name of the plugin that has started.
    pub fn current_plugin_name(&self) -> PluginName {
        self.current_plugin.clone()
    }

    /// Returns a handle that allows to send commands to control the measurement pipeline
    /// while it is running.
    pub fn pipeline_control(&self) -> pipeline::control::ScopedControlHandle {
        self.pipeline.control_handle().scoped(self.current_plugin.clone())
    }

    /// Returns a handle that allows to register new metrics while the pipeline is running,
    /// and to subscribe to new registrations.
    pub fn metrics_sender(&self) -> pipeline::registry::MetricSender {
        self.pipeline.metrics_sender()
    }

    /// Returns a read-only access to the [`MetricRegistry`].
    pub fn metrics_reader(&self) -> pipeline::registry::MetricReader {
        self.pipeline.metrics_reader()
    }

    /// Returns a handle to the main asynchronous runtime used by the pipeline.
    pub fn async_runtime(&self) -> tokio::runtime::Handle {
        self.pipeline.async_runtime().clone()
    }
}
