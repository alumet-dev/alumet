//! Phases of the plugins lifecycle.
use std::marker::PhantomData;

use crate::metrics::def::{Metric, RawMetricId, TypedMetricId};
use crate::metrics::error::MetricCreationError;
use crate::metrics::online::listener::{
    MetricListener, MetricListenerBuildContext, MetricListenerBuilder, MetricListenerRegistration,
};
use crate::metrics::online::{MetricReader, MetricSender};
use crate::metrics::registry::MetricRegistry;
use crate::pipeline::elements::{output, source, transform};
use crate::{
    measurement::{MeasurementType, WrappedMeasurementType},
    pipeline::{self, trigger, Output, PluginName, Source, Transform},
    units::PrefixedUnit,
};

/// Structure passed to plugins for the start-up phase.
///
/// It allows the plugins to perform some actions before starting the measurement pipeline,
/// such as registering new measurement sources.
///
/// # Note for applications
/// You cannot create `AlumetPluginStart` manually, build an agent with [`agent::Builder`](crate::agent::Builder) instead.
pub struct AlumetPluginStart<'a> {
    pub(crate) current_plugin: PluginName,
    pub(crate) pipeline_builder: &'a mut pipeline::Builder,
    pub(crate) pre_start_actions: &'a mut Vec<(PluginName, Box<dyn PreStartAction>)>,
    pub(crate) post_start_actions: &'a mut Vec<(PluginName, Box<dyn PostStartAction>)>,
}

pub trait PostStartAction: FnOnce(&mut AlumetPostStart) -> anyhow::Result<()> {}
impl<F> PostStartAction for F where F: FnOnce(&mut AlumetPostStart) -> anyhow::Result<()> {}

pub trait PreStartAction: FnOnce(&mut AlumetPreStart) -> anyhow::Result<()> {}
impl<F> PreStartAction for F where F: FnOnce(&mut AlumetPreStart) -> anyhow::Result<()> {}

impl<'a> AlumetPluginStart<'a> {
    /// Returns the name of the plugin that is being started.
    fn current_plugin_name(&self) -> PluginName {
        self.current_plugin.clone()
    }

    /// Creates a new metric with a measurement type `T` (checked at compile time).
    /// Fails if a metric with the same name already exists.
    ///
    /// # Example
    /// ```no_run
    /// use alumet::units::{Unit, PrefixedUnit};
    /// use alumet::metrics::TypedMetricId;
    /// # use alumet::plugin::AlumetPluginStart;
    ///
    /// # fn f() -> anyhow::Result<()> {
    /// # let alumet: &AlumetPluginStart = todo!();
    /// let proc_exec_time: TypedMetricId<u64> = alumet
    ///     .create_metric("process_execution_time", Unit::Second, "execution time of a process")?;
    ///
    /// let ram_power: TypedMetricId<u64> = alumet
    ///     .create_metric("ram_electrical_power", PrefixedUnit::milli(Unit::Watt), "instantaneous power consumption of a memory module")?;
    ///
    /// # }
    /// ```
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

    /// Adds a _managed_ measurement source to the Alumet pipeline.
    pub fn add_source(&mut self, source: Box<dyn Source>, trigger: trigger::TriggerSpec) {
        let plugin = self.current_plugin_name();
        let builder = |ctx: &mut dyn source::builder::ManagedSourceBuildContext| {
            Ok(source::builder::ManagedSourceRegistration {
                name: ctx.source_name(""),
                trigger_spec: trigger,
                source,
            })
        };
        self.pipeline_builder
            .add_source_builder(plugin, source::builder::SourceBuilder::Managed(Box::new(builder)))
    }

    /// Adds the builder of a _managed_ measurement source to the Alumet pipeline.
    ///
    /// Unlike [`add_source`](Self::add_source), the source is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the source. A good use case is to access the late registration of metrics.
    ///
    /// The downside is a more complicated code.
    /// In general, you should prefer to use [`add_source`](Self::add_source) if possible.
    pub fn add_source_builder<F: source::builder::ManagedSourceBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_source_builder(plugin, source::builder::SourceBuilder::Managed(Box::new(builder)));
    }

    /// Adds the builder of an _autonomous_ source to the Alumet pipeline.
    ///
    /// # Autonomous sources
    /// An autonomous source is not triggered by Alumet, but runs independently.
    /// It is given a [`Sender`](tokio::sync::mpsc::Sender) to send its measurements
    /// to the rest of the Alumet pipeline (transforms and outputs).
    ///
    /// # Graceful shutdown
    /// To stop the autonomous source, a [`CancellationToken`](tokio_util::sync::CancellationToken) is provided.
    /// When the token is cancelled, you should stop the source.
    ///
    /// # Example
    /// ```no_run
    /// use std::time::SystemTime;
    /// use alumet::measurement::{MeasurementBuffer, MeasurementPoint, Timestamp};
    /// use alumet::units::Unit;
    /// use alumet::pipeline::elements::source::builder::AutonomousSourceRegistration;
    /// # use alumet::plugin::AlumetPluginStart;
    ///
    /// # let alumet: &AlumetPluginStart = todo!();
    /// let metric = alumet.create_metric::<u64>("my_metric", Unit::Second, "...").unwrap();
    /// alumet.add_autonomous_source_builder(move |ctx, cancel_token, tx| {
    ///     let out_tx = tx.clone();
    ///     let source = Box::pin(async move {
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
    ///     });
    ///     Ok(AutonomousSourceRegistration {
    ///         name: ctx.source_name("my-source"),
    ///         source,
    ///     })
    /// })
    /// ```
    pub fn add_autonomous_source_builder<F: source::builder::AutonomousSourceBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_source_builder(plugin, source::builder::SourceBuilder::Autonomous(Box::new(builder)));
    }

    /// Adds a transform step to the Alumet pipeline.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use alumet::pipeline::elements::transform::{Transform, TransformContext};
    /// use alumet::pipeline::elements::error::TransformError;
    /// use alumet::measurement::MeasurementBuffer;
    /// # use alumet::plugin::AlumetPluginStart;
    ///
    /// // Define the transform
    /// struct ExampleTransform;
    /// impl Transform for ExampleTransform {
    ///     fn apply(&mut self, m: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError> {
    ///         todo!(); // do something with the measurements
    ///         Ok(())
    ///     }
    /// }
    ///
    /// # let alumet: &AlumetPluginStart = todo!();
    /// #
    /// // In start(&mut self, alumet: &mut AlumetPluginStart),
    /// // add the transform to the pipeline.
    /// let transform = ExampleTransform;
    /// alumet.add_transform(Box::new(transform));
    /// ```
    pub fn add_transform(&mut self, transform: Box<dyn Transform>) {
        let plugin = self.current_plugin_name();
        let builder = |ctx: &mut dyn transform::builder::TransformBuildContext| {
            Ok(transform::builder::TransformRegistration {
                name: ctx.transform_name(""),
                transform,
            })
        };
        self.pipeline_builder.add_transform_builder(plugin, Box::new(builder));
    }

    /// Adds the builder of a transform step to the Alumet pipeline.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use alumet::pipeline::elements::transform::{
    ///     Transform,
    ///     builder::TransformRegistration
    /// };
    ///
    /// # use alumet::plugin::AlumetPluginStart;
    ///
    /// # let alumet: &AlumetPluginStart = todo!();
    /// alumet.add_transform_builder(move |ctx| {
    ///     let name = ctx.transform_name("example");
    ///     let transform: Box<dyn Transform> = todo!();
    ///     Ok(TransformRegistration { name, transform })
    /// });
    /// ```
    pub fn add_transform_builder<F: transform::builder::TransformBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder.add_transform_builder(plugin, Box::new(builder));
    }

    /// Adds a _blocking_ output to the Alumet pipeline.
    ///
    /// # Example
    /// ```no_run
    /// use alumet::pipeline::elements::output::{Output, OutputContext};
    /// use alumet::pipeline::elements::error::WriteError;
    /// use alumet::measurement::MeasurementBuffer;
    /// # use alumet::plugin::AlumetPluginStart;
    ///
    /// use anyhow::Context;
    ///
    /// // Define the output
    /// struct ExampleOutput;
    /// impl Output for ExampleOutput {
    ///     fn write(&mut self, m: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
    ///         // do something with the measurements
    ///         for point in m.iter() {
    ///             todo!()
    ///         }
    ///         Ok(())
    ///     }
    /// }
    ///
    /// # let alumet: &AlumetPluginStart = todo!();
    /// #
    /// // In start(&mut self, alumet: &mut AlumetPluginStart),
    /// // add the output to the pipeline.
    /// let output = ExampleOutput;
    /// alumet.add_blocking_output(Box::new(output));
    /// ```
    pub fn add_blocking_output(&mut self, output: Box<dyn Output>) {
        let plugin = self.current_plugin_name();
        let build = |ctx: &mut dyn output::builder::BlockingOutputBuildContext| {
            Ok(output::builder::BlockingOutputRegistration {
                name: ctx.output_name(""),
                output,
            })
        };
        let builder = output::builder::OutputBuilder::Blocking(Box::new(build));
        self.pipeline_builder.add_output_builder(plugin, builder);
    }

    /// Adds the builder of a _blocking_ output to the Alumet pipeline.
    ///
    /// Unlike [`add_blocking_output`](Self::add_blocking_output), the output is not created immediately but during the construction
    /// of the measurement pipeline. This allows to use some information about the pipeline while
    /// creating the output.
    ///
    /// # Async outputs
    /// If you intend to use async functions to implement your output, consider using [`add_async_output_builder`](Self::add_async_output_builder)
    /// instead.
    pub fn add_blocking_output_builder<F: output::builder::BlockingOutputBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        let builder = output::builder::OutputBuilder::Blocking(Box::new(builder));
        self.pipeline_builder.add_output_builder(plugin, builder);
    }

    /// Adds the builder of an _async_ output to the Alumet pipeline.
    pub fn add_async_output_builder<F: output::builder::AsyncOutputBuilder + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        let builder = output::builder::OutputBuilder::Async(Box::new(builder));
        self.pipeline_builder.add_output_builder(plugin, builder);
    }

    /// Registers a callback that will run just after the pipeline startup.
    ///
    /// If you have some data to move to the pipeline start phase, it's easier
    /// to use this method than [`crate::plugin::Plugin::post_pipeline_start`].
    ///
    /// # Example
    /// ```no_run
    /// # use alumet::plugin::AlumetPluginStart;
    /// # let alumet: &AlumetPluginStart = todo!();
    /// alumet.on_pipeline_start(|ctx| {
    ///     // ctx is a `&mut AlumetPostStart`
    ///     let control_handle = ctx.pipeline_control();
    ///     todo!();
    ///     Ok(())
    /// })
    /// ```
    pub fn on_pipeline_start<F: PostStartAction + 'static>(&mut self, action: F) {
        let plugin = self.current_plugin_name();
        self.post_start_actions.push((plugin, Box::new(action)));
    }

    /// Registers a callback that will run just before the pipeline startup.
    ///
    /// If you have some data to move to the pipeline start phase, it's easier
    /// to use this method than [`crate::plugin::Plugin::pre_pipeline_start`].
    pub fn on_pre_pipeline_start<F: PreStartAction + 'static>(&mut self, action: F) {
        let plugin = self.current_plugin_name();
        self.pre_start_actions.push((plugin, Box::new(action)));
    }
}

/// Structure passed to plugins for the pre start-up phase.
pub struct AlumetPreStart<'a> {
    pub(crate) current_plugin: PluginName,
    pub(crate) pipeline_builder: &'a mut pipeline::Builder,
}

impl<'a> AlumetPreStart<'a> {
    /// Returns the name of the plugin that has started.
    pub fn current_plugin_name(&self) -> PluginName {
        self.current_plugin.clone()
    }

    /// Returns a read-only access to the [`MetricRegistry`].
    pub fn metrics(&self) -> &MetricRegistry {
        &self.pipeline_builder.metrics
    }

    /// Registers a metric listener, which will be notified of all the new registered metrics.
    pub fn add_metric_listener<F: MetricListener + Send + 'static>(&mut self, listener: F) {
        let builder = |ctx: &mut dyn MetricListenerBuildContext| {
            Ok(MetricListenerRegistration {
                name: ctx.listener_name(""),
                listener: Box::new(listener),
            })
        };
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_metric_listener_builder(plugin, Box::new(builder));
    }

    /// Registers a metric listener builder, which will construct a listener that
    /// will be notified of all the new registered metrics.
    pub fn add_metric_listener_builder<F: MetricListenerBuilder + Send + 'static>(&mut self, builder: F) {
        let plugin = self.current_plugin_name();
        self.pipeline_builder
            .add_metric_listener_builder(plugin, Box::new(builder));
    }
}

/// Structure passed to plugins for the post start-up phase.
pub struct AlumetPostStart<'a> {
    pub(crate) current_plugin: PluginName,
    pub(crate) pipeline: &'a mut pipeline::MeasurementPipeline,
}

impl<'a> AlumetPostStart<'a> {
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
    pub fn metrics_sender(&self) -> MetricSender {
        self.pipeline.metrics_sender()
    }

    /// Returns a read-only access to the [`MetricRegistry`].
    pub fn metrics_reader(&self) -> MetricReader {
        self.pipeline.metrics_reader()
    }

    /// Returns a handle to the main asynchronous runtime used by the pipeline.
    pub fn async_runtime(&self) -> tokio::runtime::Handle {
        self.pipeline.async_runtime().clone()
    }
}
