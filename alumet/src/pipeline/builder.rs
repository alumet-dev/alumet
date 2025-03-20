//! Construction of measurement pipelines.
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use fxhash::FxHashMap;
use tokio::{
    runtime::Runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::listener::MetricListenerBuilder;
use crate::metrics::online::{MetricReader, MetricRegistryControl, MetricSender};
use crate::metrics::registry::MetricRegistry;
use crate::pipeline::elements::output::control::OutputControl;
use crate::pipeline::elements::output::OutputContext;
use crate::pipeline::elements::source::control::SourceControl;
use crate::pipeline::elements::transform::control::TransformControl;
use crate::pipeline::util::channel;
use crate::pipeline::Output;

use super::elements::output::builder::OutputBuilder;
use super::elements::source::builder::SourceBuilder;
use super::elements::source::trigger::TriggerConstraints;
use super::elements::transform::builder::TransformBuilder;
use super::error::PipelineError;
use super::naming::{
    namespace::{DuplicateNameError, Namespace2},
    OutputName, PluginName, SourceName, TransformName,
};
use super::{
    control::key::{OutputKey, SourceKey, TransformKey},
    control::{AnonymousControlHandle, PipelineControl},
    util,
};

/// A running measurement pipeline.
pub struct MeasurementPipeline {
    rt_normal: Runtime,
    _rt_priority: Option<Runtime>,
    control_handle: AnonymousControlHandle,
    metrics: (MetricSender, MetricReader),
    pipeline_control_task: JoinHandle<Result<(), PipelineError>>,
    metrics_control_task: JoinHandle<()>,
}

/// A Builder for [`MeasurementPipeline`].
///
/// This type allows to configure the measurement pipeline bit by bit.
/// It is usually more practical not to call [`build`](Self::build) but to use the [`agent::Builder`](crate::agent::Builder) instead.
pub struct Builder {
    // Pipeline elements, by plugin and name. The tuple (plugin, element name) is enforced to be unique.
    sources: Namespace2<SourceBuilder>,
    transforms: Namespace2<Box<dyn TransformBuilder>>,
    outputs: Namespace2<OutputBuilder>,

    /// Order of the transforms, manually specified.
    transforms_order: Option<Vec<TransformName>>,
    /// Order in which the transforms have been added, to use if `transforms_order` is `None`.
    default_transforms_order: Vec<TransformName>,

    /// Constraints to apply to the TriggerSpec of managed sources.
    trigger_constraints: TriggerConstraints,

    /// How many `MeasurementBuffer` can be stored in the channel that sources write to.
    source_channel_size: usize,

    /// Metrics
    pub(crate) metrics: MetricRegistry,
    metric_listeners: Namespace2<Box<dyn MetricListenerBuilder>>,

    // tokio::Runtime settings.
    threads_normal: Option<usize>,
    threads_high_priority: Option<usize>,
}

/// Allows to inspect the content of a pipeline builder.
pub struct BuilderInspector<'a> {
    inner: &'a Builder,
}

const DEFAULT_CHAN_BUF_SIZE: usize = 2048;

impl Builder {
    pub fn new() -> Self {
        Self {
            sources: Namespace2::new(),
            transforms: Namespace2::new(),
            outputs: Namespace2::new(),
            transforms_order: None,
            default_transforms_order: Vec::new(),
            trigger_constraints: TriggerConstraints::default(),
            source_channel_size: DEFAULT_CHAN_BUF_SIZE,
            metrics: MetricRegistry::new(),
            metric_listeners: Namespace2::new(),
            threads_normal: None, // default to the number of cores
            threads_high_priority: None,
        }
    }

    /// Returns a mutable reference to the constraints that will be applied, on build,
    /// to every registered trigger (i.e. to all managed sources).
    pub fn trigger_constraints_mut(&mut self) -> &mut TriggerConstraints {
        &mut self.trigger_constraints
    }

    /// Returns a mutable reference to the size of the channel that sources write to.
    ///
    /// This number limits how many [`MeasurementBuffer`] can be stored in the channel buffer.
    /// You may want to increase this if you get `buffer is full` errors, which can happen
    /// if you have a large number of sources that flush at the same time.
    pub fn source_channel_size(&mut self) -> &mut usize {
        &mut self.source_channel_size
    }

    /// Registers a listener that will be notified of the metrics that are created while the pipeline is running,
    /// with a dedicated builder.
    pub fn add_metric_listener_builder(
        &mut self,
        plugin: PluginName,
        name: &str,
        builder: Box<dyn MetricListenerBuilder>,
    ) -> Result<(), DuplicateNameError> {
        self.metric_listeners.add(plugin.0, name.to_owned(), builder)
    }

    /// Adds a source to the pipeline, with a dedicated builder.
    pub fn add_source_builder(
        &mut self,
        plugin: PluginName,
        name: &str,
        builder: SourceBuilder,
    ) -> Result<SourceKey, DuplicateNameError> {
        match self.sources.add(plugin.0.clone(), name.to_owned(), builder) {
            Ok(_) => Ok(SourceKey::new(SourceName::new(plugin.0, name.to_owned()))),
            Err(e) => Err(e),
        }
    }

    /// Adds a transform function to the pipeline, with a dedicated builder.
    pub fn add_transform_builder(
        &mut self,
        plugin: PluginName,
        name: &str,
        builder: Box<dyn TransformBuilder>,
    ) -> Result<TransformKey, DuplicateNameError> {
        match self.transforms.add(plugin.0.clone(), name.to_owned(), builder) {
            Ok(_) => {
                let name = TransformName::new(plugin.0, name.to_owned());
                self.default_transforms_order.push(name.clone());
                Ok(TransformKey::new(name))
            }
            Err(e) => Err(e),
        }
    }

    /// Adds an output to the pipeline, with a dedicated builder.
    pub fn add_output_builder(
        &mut self,
        plugin: PluginName,
        name: &str,
        builder: OutputBuilder,
    ) -> Result<OutputKey, DuplicateNameError> {
        match self.outputs.add(plugin.0.clone(), name.to_owned(), builder) {
            Ok(_) => Ok(OutputKey::new(OutputName::new(plugin.0, name.to_owned()))),
            Err(e) => Err(e),
        }
    }

    /// Sets the number of non-high-priority threads to use.
    ///
    /// # Default
    /// The default value is the number of cores available to the system.
    pub fn normal_threads(&mut self, n: usize) {
        self.threads_normal = Some(n);
    }

    /// Sets the number of high-priority threads to use.
    ///
    /// # Default
    /// The default value is the number of cores available to the system.
    pub fn high_priority_threads(&mut self, n: usize) {
        self.threads_high_priority = Some(n);
    }

    /// Sets the execution order of the transforms.
    ///
    /// If this method is not called, the default order is the one
    /// in which the transform builders have been added to the builder.
    pub fn transforms_order(&mut self, order: Vec<TransformName>) {
        self.transforms_order = Some(order);
    }

    /// Replaces each source builder with the result of the closure `f`.
    pub fn replace_sources(&mut self, mut f: impl FnMut(SourceName, SourceBuilder) -> SourceBuilder) {
        self.sources.replace_each(|(plugin, source), builder| {
            let name = SourceName::new(plugin.to_owned(), source.to_owned());
            f(name, builder)
        });
    }

    /// Replaces each transform builder with the result of the closure `f`.
    pub fn replace_transforms(
        &mut self,
        mut f: impl FnMut(TransformName, Box<dyn TransformBuilder>) -> Box<dyn TransformBuilder>,
    ) {
        self.transforms.replace_each(|(plugin, transform), builder| {
            let name = TransformName::new(plugin.to_owned(), transform.to_owned());
            f(name, builder)
        });
    }

    /// Replaces each output builder with the result of the closure `f`.
    pub fn replace_outputs(&mut self, mut f: impl FnMut(OutputName, OutputBuilder) -> OutputBuilder) {
        self.outputs.replace_each(|(plugin, output), builder| {
            let name = OutputName::new(plugin.to_owned(), output.to_owned());
            f(name, builder)
        });
    }

    /// Builds the measurement pipeline.
    ///
    /// The new pipeline is immediately started.
    pub fn build(mut self) -> anyhow::Result<MeasurementPipeline> {
        /// Adds a dummy output builder to `outputs`.
        ///
        /// The dummy output does nothing with the measurements.
        fn add_dummy_output(outputs: &mut Namespace2<OutputBuilder>) {
            use crate::pipeline::elements::output::error::WriteError;

            struct DummyOutput;
            impl Output for DummyOutput {
                fn write(&mut self, _m: &MeasurementBuffer, _ctx: &OutputContext) -> Result<(), WriteError> {
                    Ok(())
                }
            }
            let builder = OutputBuilder::Blocking(Box::new(|_| Ok(Box::new(DummyOutput))));
            outputs
                .add(String::from("alumet"), String::from("dummy"), builder)
                .unwrap();
        }

        /// Take the builders out of `transforms` by following the given `order`.
        fn take_transforms_in_order(
            mut transforms: Namespace2<Box<dyn TransformBuilder>>,
            order: Vec<TransformName>,
        ) -> anyhow::Result<Vec<(TransformName, Box<dyn TransformBuilder>)>> {
            let res = order
                .into_iter()
                .map(|name| {
                    transforms
                        .remove(name.plugin(), name.transform())
                        .ok_or_else(|| anyhow!("an order was specified for a transform that does not exist: {name}"))
                        .map(|builder| (name, builder))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            if !transforms.is_empty() {
                let names = transforms
                    .flat_keys()
                    .map(|(plugin, trans)| format!("{plugin}/{trans}"))
                    .collect::<Vec<String>>()
                    .join(", ");
                return Err(anyhow!("missing order for these transforms: {names}"));
            }
            Ok(res)
        }

        // Tokio runtime backed by "real-time" high priority threads.
        let rt_priority: Option<Runtime> = if self.threads_high_priority == Some(0) {
            None
        } else {
            util::threading::build_priority_runtime(self.threads_high_priority).ok()
        };

        // Tokio runtime backed by usual threads (default priority).
        let rt_normal: Runtime = {
            let n_threads = if let Some(n) = self.threads_normal {
                Some(n)
            } else if rt_priority.is_some() && self.threads_high_priority.is_none() {
                Some(2)
            } else {
                None
            };
            util::threading::build_normal_runtime(n_threads).context("could not build the multithreaded Runtime")?
        };
        let rt_handle = rt_normal.handle();

        // Token to initiate the shutdown of the pipeline, before the elements have been stopped.
        let pipeline_shutdown = CancellationToken::new();

        // Token to shutdown the remaining parts of the pipeline, after the elements have been stopped.
        let pipeline_shutdown_finalize = CancellationToken::new();

        // --- Metric registry (one for the entire pipeline) ---
        // Note: We can modify it without sending a message thanks to MetricAccess::write().
        let mut registry_control = MetricRegistryControl::new(self.metrics);

        // Before it starts, register the initial listeners.
        registry_control
            .create_listeners(self.metric_listeners, rt_handle)
            .context("could not create the metric listeners")?;

        // Start the MetricRegistryControl
        // Stop it once the pipeline elements have shut down.
        let (metrics_tx, metrics_rw, metrics_join) =
            registry_control.start(pipeline_shutdown_finalize.child_token(), rt_handle);
        let metrics_r = metrics_rw.into_read_only();

        // --- Build the pipeline elements and control loops, with some optimizations ---

        // Channel: sources -> transforms (or sources -> output in case of optimization).
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(self.source_channel_size);

        let mut output_control;
        let transform_control;

        if self.outputs.is_empty() {
            log::warn!("No output has been registered. A dummy output will be added to make the pipeline work, but you probably want to add a true output.");
            add_dummy_output(&mut self.outputs);
        }

        if self.outputs.total_count() == 1 && self.transforms.is_empty() {
            // OPTIMIZATION: there is only one output and no transform,
            // we can connect the inputs directly to the output.
            log::info!("Only one output and no transform, using a simplified and optimized measurement pipeline.");

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(in_rx);
            output_control = OutputControl::new(out_rx_provider, rt_handle.clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // No transforms
            transform_control = TransformControl::empty();
        } else {
            // Broadcast queue: transforms -> outputs
            let out_tx = broadcast::Sender::<MeasurementBuffer>::new(self.source_channel_size);

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(out_tx.clone());
            output_control = OutputControl::new(out_rx_provider, rt_handle.clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // Transforms
            let order = self.transforms_order.unwrap_or(self.default_transforms_order);
            let transforms = take_transforms_in_order(self.transforms, order)?;
            transform_control =
                TransformControl::with_transforms(transforms, metrics_r.clone(), in_rx, out_tx, rt_handle)?;
        };

        // Sources, last in order not to loose any measurement if they start measuring right away.
        let mut source_control = SourceControl::new(
            self.trigger_constraints,
            pipeline_shutdown.clone(),
            in_tx,
            rt_handle.clone(),
            rt_priority.as_ref().unwrap_or(&rt_normal).handle().clone(),
            (metrics_r.clone(), metrics_tx.clone()),
        );
        source_control
            .blocking_create_sources(self.sources)
            .context("source creation failed")?;

        // Pipeline control
        let control = PipelineControl::new(source_control, transform_control, output_control);
        let (control_handle, control_join) = control.start(pipeline_shutdown, pipeline_shutdown_finalize, rt_handle);

        // Done!
        Ok(MeasurementPipeline {
            rt_normal,
            _rt_priority: rt_priority,
            control_handle,
            metrics: (metrics_tx, metrics_r),
            pipeline_control_task: control_join,
            metrics_control_task: metrics_join,
        })
    }

    /// Inspects the current state of the builder.
    ///
    /// # Example
    /// ```
    /// use alumet::pipeline;
    /// use alumet::pipeline::naming::SourceName;
    ///
    /// # use alumet::pipeline::naming::PluginName;
    /// # use alumet::pipeline::elements::source::builder::SourceBuilder;
    /// # fn f(plugin: PluginName, source: SourceBuilder) {
    ///
    /// // Create a pipeline builder.
    /// let mut pipeline = pipeline::Builder::new();
    ///
    /// // Register a source.
    /// pipeline.add_source_builder(plugin, "example", source);
    ///
    /// // Get the number of sources.
    /// let inspect = pipeline.inspect();
    /// let n_sources = inspect.stats().sources;
    /// assert_eq!(n_sources, 1);
    ///
    /// // Get the names of the sources.
    /// let names = inspect.sources();
    /// assert_eq!(names[0].source(), "example");
    ///
    /// # }
    /// ```
    pub fn inspect(&self) -> BuilderInspector {
        BuilderInspector { inner: self }
    }
}

/// Statistics about the current state of the builder.
pub struct BuilderStats {
    /// Number of registered source builders.
    pub sources: usize,
    /// Number of registered transform builders.
    pub transforms: usize,
    /// Number of registered output builders.
    pub outputs: usize,
    /// Number of registered metrics.
    pub metrics: usize,
    /// Number of registered metric listeners.
    pub metric_listeners: usize,
}

impl<'a> BuilderInspector<'a> {
    /// Returns a read-only access to the current state of the metric registry.
    pub fn metrics(&self) -> &MetricRegistry {
        &self.inner.metrics
    }

    /// Returns statistics about the builder: how many sources, transforms, etc.
    pub fn stats(&self) -> BuilderStats {
        BuilderStats {
            sources: self.inner.sources.total_count(),
            transforms: self.inner.transforms.total_count(),
            outputs: self.inner.outputs.total_count(),
            metrics: self.inner.metrics.len(),
            metric_listeners: self.inner.metric_listeners.total_count(),
        }
    }

    /// Lists the names of the registered sources.
    pub fn sources(&self) -> Vec<SourceName> {
        self.inner
            .sources
            .flat_keys()
            .map(|(plugin, source)| SourceName::new(plugin.to_owned(), source.to_owned()))
            .collect()
    }

    /// Lists the names of the registered sources, grouped by plugin.
    pub fn sources_by_plugin(&self) -> impl Iterator<Item = (PluginName, Vec<SourceName>)> {
        // Returns impl Iterator because we don't want to commit to FxHashMap.
        let mut map: FxHashMap<PluginName, Vec<SourceName>> = FxHashMap::default();
        for (plugin, source) in self.inner.sources.flat_keys() {
            let key = PluginName(plugin.to_owned());
            let name = SourceName::new(plugin.to_owned(), source.to_owned());
            map.entry(key).or_insert(Default::default()).push(name);
        }
        map.into_iter()
    }

    /// Lists the names of the registered transforms.
    pub fn transforms(&self) -> Vec<TransformName> {
        self.inner
            .transforms
            .flat_keys()
            .map(|(plugin, transform)| TransformName::new(plugin.to_owned(), transform.to_owned()))
            .collect()
    }

    /// Lists the names of the registered transforms, grouped by plugin.
    pub fn transforms_by_plugin(&self) -> impl Iterator<Item = (PluginName, Vec<TransformName>)> {
        let mut map: FxHashMap<PluginName, Vec<TransformName>> = FxHashMap::default();
        for (plugin, transform) in self.inner.transforms.flat_keys() {
            let key = PluginName(plugin.to_owned());
            let name = TransformName::new(plugin.to_owned(), transform.to_owned());
            map.entry(key).or_insert(Default::default()).push(name);
        }
        map.into_iter()
    }

    /// Lists the names of the registered outputs.
    pub fn outputs(&self) -> Vec<OutputName> {
        self.inner
            .outputs
            .flat_keys()
            .map(|(plugin, output)| OutputName::new(plugin.to_owned(), output.to_owned()))
            .collect()
    }

    /// Lists the names of the registered outputs, grouped by plugin.
    pub fn outputs_by_plugin(&self) -> impl Iterator<Item = (PluginName, Vec<OutputName>)> {
        let mut map: FxHashMap<PluginName, Vec<OutputName>> = FxHashMap::default();
        for (plugin, output) in self.inner.outputs.flat_keys() {
            let key = PluginName(plugin.to_owned());
            let name = OutputName::new(plugin.to_owned(), output.to_owned());
            map.entry(key).or_insert(Default::default()).push(name);
        }
        map.into_iter()
    }
}

/// An error was detected while shutting the pipeline down.
///
/// This does NOT mean that the shutdown failed.
/// The error could have happened while the pipeline was running.
/// Most errors do not terminate the pipeline, they just stop the failed element.
pub enum ShutdownError {
    /// An error occurred in the pipeline.
    ///
    /// Use methods like [`PipelineError::is_internal`] to differentiate between internal
    /// pipeline errors (which should not happen) and errors that originated from a pipeline
    /// element (such as a source).
    Pipeline(PipelineError),
    /// Shutdown timeout expired.
    TimeoutExpired,
}

impl MeasurementPipeline {
    /// Returns a _control handle_, which allows to send control commands to the pipeline
    /// (in order to modify its configuration) and to shut it down.
    pub fn control_handle(&self) -> AnonymousControlHandle {
        self.control_handle.clone()
    }

    /// Returns a read-only access to the pipeline's metric registry.
    pub fn metrics_reader(&self) -> MetricReader {
        self.metrics.1.clone()
    }

    /// Returns a `MetricSender` that allows to register new metrics while the pipeline is running.
    pub fn metrics_sender(&self) -> MetricSender {
        self.metrics.0.clone()
    }

    /// Returns a handle to the non-high-priority tokio async runtime.
    ///
    /// This handle can be used to start asynchronous tasks that will be cancelled when
    /// the pipeline is shut down. It also avoids to create a separate async runtime.
    pub fn async_runtime(&self) -> &tokio::runtime::Handle {
        self.rt_normal.handle()
    }

    /// Wait for the pipeline to be shut down (via its [`control_handle()`](Self::control_handle) or by `Ctrl+C`).
    ///
    /// # Blocking
    /// This is a blocking function, it should not be called from within an async runtime.
    pub fn wait_for_shutdown(self, timeout: Option<Duration>) -> Result<(), ShutdownError> {
        log::debug!("pipeline::wait_for_shutdown");
        let shutdown_task = async {
            let pipeline_result = self
                .pipeline_control_task
                .await
                .context("pipeline_control_task failed to execute to completion")?;

            log::trace!("pipeline_control_task has ended, waiting for metrics_control_task");
            self.metrics_control_task
                .await
                .context("metrics_control_task failed to execute to completion")?;

            log::trace!("metrics_control_task has ended, dropping the pipeline");
            pipeline_result
        };

        if let Some(timeout) = timeout {
            // It is necessary to wrap the timeout in a new async block, because it needs
            // to be constructed in the context of a Runtime.
            let t0 = Instant::now();
            let res = self
                .rt_normal
                .block_on(async { tokio::time::timeout(timeout, shutdown_task).await });

            match res {
                Ok(res) => {
                    // Try to shutdown the runtime with a timeout.
                    // In any case, the runtime will be dropped at the end of this method,
                    // which will call `shutdown` without any timeout, but it will not hang indefinitely:
                    // the second shutdown will do nothing, because we already have initiated a shutdown.
                    let t1 = Instant::now();
                    let remaining_time = (t1 - t0).saturating_sub(timeout);
                    self.rt_normal.shutdown_timeout(remaining_time);
                    if let Some(rt_priority) = self._rt_priority {
                        let t2 = Instant::now();
                        let remaining_time = (t2 - t0).saturating_sub(timeout);
                        rt_priority.shutdown_timeout(remaining_time);
                    }
                    let t_end = Instant::now();
                    if t_end - t0 <= timeout {
                        res.map_err(ShutdownError::Pipeline)
                    } else {
                        Err(ShutdownError::TimeoutExpired)
                    }
                }
                Err(_) => Err(ShutdownError::TimeoutExpired),
            }
        } else {
            self.rt_normal.block_on(shutdown_task).map_err(ShutdownError::Pipeline)
        }
    }
}
