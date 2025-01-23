//! Construction of measurement pipelines.
use std::time::Duration;

use super::elements::{output, source, transform};
use super::registry::listener::MetricListenerBuilder;
use super::registry::{MetricReader, MetricSender};
use crate::pipeline::registry::MetricRegistryControl;
use crate::pipeline::util::channel;
use crate::{measurement::MeasurementBuffer, metrics::MetricRegistry};

use super::util::naming::PluginName;
use super::{
    control::{AnonymousControlHandle, PipelineControl},
    trigger::TriggerConstraints,
    util,
};
use anyhow::Context;
use tokio::time::error::Elapsed;
use tokio::{
    runtime::Runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

/// A running measurement pipeline.
pub struct MeasurementPipeline {
    rt_normal: Runtime,
    _rt_priority: Option<Runtime>,
    control_handle: AnonymousControlHandle,
    metrics: (MetricSender, MetricReader),
    pipeline_control_task: JoinHandle<()>,
    metrics_control_task: JoinHandle<()>,
}

/// A Builder for [`MeasurementPipeline`].
///
/// This type allows to configure the measurement pipeline bit by bit.
/// It is usually more practical not to call [`build`](Self::build) but to use the [`agent::Builder`](crate::agent::Builder) instead.
pub struct Builder {
    sources: Vec<(PluginName, source::builder::SourceBuilder)>,
    transforms: Vec<(PluginName, Box<dyn transform::builder::TransformBuilder>)>,
    outputs: Vec<(PluginName, output::builder::OutputBuilder)>,

    /// Constraints to apply to the TriggerSpec of managed sources.
    trigger_constraints: TriggerConstraints,

    /// How many `MeasurementBuffer` can be stored in the channel that sources write to.
    source_channel_size: usize,

    /// Metrics
    pub(crate) metrics: MetricRegistry,
    metric_listeners: Vec<(PluginName, Box<dyn MetricListenerBuilder>)>,

    threads_normal: Option<usize>,
    threads_high_priority: Option<usize>,
}

const DEFAULT_CHAN_BUF_SIZE: usize = 2048;

impl Builder {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            trigger_constraints: TriggerConstraints::default(),
            source_channel_size: DEFAULT_CHAN_BUF_SIZE,
            metrics: MetricRegistry::new(),
            metric_listeners: Vec::new(),
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
    pub fn add_metric_listener_builder(&mut self, plugin: PluginName, builder: Box<dyn MetricListenerBuilder>) {
        self.metric_listeners.push((plugin, builder))
    }

    /// Adds a source to the pipeline, with a dedicated builder.
    pub fn add_source_builder(&mut self, plugin: PluginName, builder: source::builder::SourceBuilder) {
        self.sources.push((plugin, builder))
    }

    /// Adds a transform function to the pipeline, with a dedicated builder.
    pub fn add_transform_builder(
        &mut self,
        plugin: PluginName,
        builder: Box<dyn transform::builder::TransformBuilder>,
    ) {
        self.transforms.push((plugin, builder))
    }

    /// Adds an output to the pipeline, with a dedicated builder.
    pub fn add_output_builder(&mut self, plugin: PluginName, builder: output::builder::OutputBuilder) {
        self.outputs.push((plugin, builder))
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

    /// Builds the measurement pipeline.
    ///
    /// The new pipeline is immediately started.
    pub fn build(mut self) -> anyhow::Result<MeasurementPipeline> {
        use output::builder::{BlockingOutputBuildContext, BlockingOutputRegistration};

        fn dummy_output_builder(
            ctx: &mut dyn BlockingOutputBuildContext,
        ) -> anyhow::Result<BlockingOutputRegistration> {
            use crate::pipeline::{elements::error::WriteError, Output};

            struct DummyOutput;
            impl Output for DummyOutput {
                fn write(&mut self, _m: &MeasurementBuffer, _ctx: &output::OutputContext) -> Result<(), WriteError> {
                    Ok(())
                }
            }

            Ok(BlockingOutputRegistration {
                name: ctx.output_name("dummy"),
                output: Box::new(DummyOutput),
            })
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
            let no_plugin = PluginName(String::from("_"));
            let builder = output::builder::OutputBuilder::Blocking(Box::new(dummy_output_builder));
            self.outputs.push((no_plugin, builder));
        }

        if self.outputs.len() == 1 && self.transforms.is_empty() {
            // OPTIMIZATION: there is only one output and no transform,
            // we can connect the inputs directly to the output.
            log::info!("Only one output and no transform, using a simplified and optimized measurement pipeline.");

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(in_rx);
            output_control = output::OutputControl::new(out_rx_provider, rt_handle.clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // No transforms
            transform_control = transform::TransformControl::empty();
        } else {
            // Broadcast queue: transforms -> outputs
            let out_tx = broadcast::Sender::<MeasurementBuffer>::new(self.source_channel_size);

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(out_tx.clone());
            output_control = output::OutputControl::new(out_rx_provider, rt_handle.clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // Transforms
            transform_control = transform::TransformControl::with_transforms(
                self.transforms,
                metrics_r.clone(),
                in_rx,
                out_tx,
                rt_handle,
            )?;
        };

        // Sources, last in order not to loose any measurement if they start measuring right away.
        let mut source_control = source::SourceControl::new(
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

    /// Returns statistics about the current state of the builder.
    pub fn stats(&self) -> BuilderStats {
        BuilderStats {
            sources: self.sources.len(),
            transforms: self.transforms.len(),
            outputs: self.outputs.len(),
            metrics: self.metrics.len(),
            metric_listeners: self.metric_listeners.len(),
        }
    }

    /// Returns a read-only access to the current state of the metric registry.
    pub fn metrics(&self) -> &MetricRegistry {
        &self.metrics
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
    pub fn wait_for_shutdown(self, timeout: Option<Duration>) -> Result<anyhow::Result<()>, Elapsed> {
        log::debug!("pipeline::wait_for_shutdown");
        let rt = self.rt_normal;
        let shutdown_task = async {
            self.pipeline_control_task
                .await
                .context("pipeline_control_task failed to execute to completion")?;

            log::trace!("pipeline_control_task has ended, waiting for metrics_control_task");
            self.metrics_control_task
                .await
                .context("metrics_control_task failed to execute to completion")?;

            log::trace!("metrics_control_task has ended, dropping the pipeline");
            Ok::<(), anyhow::Error>(())
        };
        if let Some(duration) = timeout {
            // It is necessary to wrap the timeout in a new async block, because it needs
            // to be constructed in the context of a Runtime.
            rt.block_on(async { tokio::time::timeout(duration, shutdown_task).await })
        } else {
            Ok(rt.block_on(shutdown_task))
        }
        // the Runtime is dropped
    }
}
