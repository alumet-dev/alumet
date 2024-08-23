//! Construction of measurement pipelines.
use std::time::Duration;

use super::elements::{output, source, transform};
use super::registry::{self, MetricReader, MetricSender};
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

/// Builder for [`MeasurementPipeline`].
pub struct Builder {
    sources: Vec<(PluginName, elements::SourceBuilder)>,
    transforms: Vec<(PluginName, Box<dyn elements::TransformBuilder>)>,
    outputs: Vec<(PluginName, Box<dyn elements::OutputBuilder>)>,

    /// Constraints to apply to the TriggerSpec of managed sources.
    trigger_constraints: TriggerConstraints,

    /// Metrics
    pub(crate) metrics: MetricRegistry,
    metric_listeners: Vec<registry::MetricListener>,

    threads_normal: Option<usize>,
    threads_high_priority: Option<usize>,
}

/// Definitions of element builders.
/// 
/// # Why are the builders traits?
/// Builders are just closures, but they are quite long and used in various places of the `alumet` crate.
/// To deduplicate the code and make it more readable, _trait aliases_ would have been idea.
/// 
/// Unfortunately, _trait aliases_ are currently unstable.
/// Therefore, I have defined subtraits with an automatic implementation for closures.
#[rustfmt::skip]
pub mod elements {
    use tokio::sync::mpsc::Sender;

    use tokio_util::sync::CancellationToken;

    use crate::{
        measurement::MeasurementBuffer,
        pipeline::{
            elements::{output, source, transform},
            trigger,
            util::naming::{OutputName, SourceName, TransformName},
        },
    };

    use super::context::*;

    // Trait aliases are unstable, and the following is not enough to help deduplicating code in plugin::phases.
    //
    //     pub type ManagedSourceBuilder = dyn FnOnce(&mut dyn SourceBuildContext) -> ManagedSourceRegistration;
    //
    // Therefore, we define a subtrait that is automatically implemented for closures.
    
    /// Trait for managed source builders.
    /// 
    /// # Example
    /// ```
    /// use alumet::pipeline::builder::elements::{ManagedSourceBuilder, ManagedSourceRegistration};
    /// use alumet::pipeline::builder::context::{SourceBuildContext};
    /// use alumet::pipeline::{trigger, Source};
    /// use std::time::Duration;
    /// 
    /// fn build_my_source() -> anyhow::Result<Box<dyn Source>> {
    ///     todo!("build a new source")
    /// }
    /// 
    /// let builder: &dyn ManagedSourceBuilder = &|ctx: &mut dyn SourceBuildContext| {
    ///     let source = build_my_source()?;
    ///     Ok(ManagedSourceRegistration {
    ///         name: ctx.source_name("my-source"),
    ///         trigger_spec: trigger::TriggerSpec::at_interval(Duration::from_secs(1)),
    ///         source,
    ///     })
    /// };
    /// ```
    pub trait ManagedSourceBuilder: FnOnce(&mut dyn SourceBuildContext) -> anyhow::Result<ManagedSourceRegistration> {}
    impl<F> ManagedSourceBuilder for F where F: FnOnce(&mut dyn SourceBuildContext) -> anyhow::Result<ManagedSourceRegistration> {}

    /// Trait for autonomous source builders.
    /// 
    /// # Example
    /// ```
    /// use alumet::pipeline::builder::elements::{AutonomousSourceBuilder, AutonomousSourceRegistration};
    /// use alumet::pipeline::builder::context::{AutonomousSourceBuildContext};
    /// use alumet::pipeline::{trigger, Source};
    /// use alumet::measurement::MeasurementBuffer;
    /// 
    /// use std::time::Duration;
    /// use tokio::sync::mpsc::Sender;
    /// use tokio_util::sync::CancellationToken;
    /// 
    /// async fn my_autonomous_source(shutdown: CancellationToken, tx: Sender<MeasurementBuffer>) -> anyhow::Result<()> {
    ///     let fut = async { todo!("async trigger") };
    ///     loop {
    ///         tokio::select! {
    ///             _ = shutdown.cancelled() => {
    ///                 // stop here
    ///                 break;
    ///             },
    ///             _ = fut => {
    ///                 todo!("measure something and send it to tx");
    ///             }
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// 
    /// let builder: &dyn AutonomousSourceBuilder = &|ctx: &mut dyn AutonomousSourceBuildContext, shutdown: CancellationToken, tx: Sender<MeasurementBuffer>| {
    ///     let source = Box::pin(my_autonomous_source(shutdown, tx));
    ///     Ok(AutonomousSourceRegistration {
    ///         name: ctx.source_name("my-autonomous-source"),
    ///         source,
    ///         // No trigger here, the source is autonomous and triggers itself.
    ///     })
    /// };
    /// ```
    pub trait AutonomousSourceBuilder:
        FnOnce(&mut dyn AutonomousSourceBuildContext, CancellationToken, Sender<MeasurementBuffer>) -> anyhow::Result<AutonomousSourceRegistration>
    {}
    impl<F> AutonomousSourceBuilder for F where
        F: FnOnce(&mut dyn AutonomousSourceBuildContext, CancellationToken, Sender<MeasurementBuffer>) -> anyhow::Result<AutonomousSourceRegistration>
    {}

    /// Trait for transform builders.
    /// 
    ///  # Example
    /// ```
    /// use alumet::pipeline::builder::elements::{TransformBuilder, TransformRegistration};
    /// use alumet::pipeline::builder::context::{TransformBuildContext};
    /// use alumet::pipeline::{trigger, Transform};
    /// 
    /// fn build_my_transform() -> anyhow::Result<Box<dyn Transform>> {
    ///     todo!("build a new transform")
    /// }
    /// 
    /// let builder: &dyn TransformBuilder = &|ctx: &mut dyn TransformBuildContext| {
    ///     let transform = build_my_transform()?;
    ///     Ok(TransformRegistration {
    ///         name: ctx.transform_name("my-transform"),
    ///         transform,
    ///     })
    /// };
    /// ```
    pub trait TransformBuilder: FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<TransformRegistration> {}
    impl<F> TransformBuilder for F where F: FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<TransformRegistration> {}

    /// Trait for output builders.
    /// 
    ///  # Example
    /// ```
    /// use alumet::pipeline::builder::elements::{OutputBuilder, OutputRegistration};
    /// use alumet::pipeline::builder::context::{OutputBuildContext};
    /// use alumet::pipeline::{trigger, Output};
    /// 
    /// fn build_my_output() -> anyhow::Result<Box<dyn Output>> {
    ///     todo!("build a new output")
    /// }
    /// 
    /// let builder: &dyn OutputBuilder = &|ctx: &mut dyn OutputBuildContext| {
    ///     let output = build_my_output()?;
    ///     Ok(OutputRegistration {
    ///         name: ctx.output_name("my-output"),
    ///         output,
    ///     })
    /// };
    /// ```
    pub trait OutputBuilder: FnOnce(&mut dyn OutputBuildContext) -> anyhow::Result<OutputRegistration> {}
    impl<F> OutputBuilder for F where F: FnOnce(&mut dyn OutputBuildContext) -> anyhow::Result<OutputRegistration> {}

    /// A source builder, for a managed or autonomous source.
    /// 
    /// Use this type in the pipeline Builder.
    pub enum SourceBuilder {
        Managed(Box<dyn ManagedSourceBuilder>),
        Autonomous(Box<dyn AutonomousSourceBuilder>),
    }

    /// Like [`SourceBuilder`] but with a [`Send`] bound on the builder.
    /// 
    /// Use this type in the pipeline control loop.
    pub enum SendSourceBuilder {
        Managed(Box<dyn ManagedSourceBuilder + Send>),
        Autonomous(Box<dyn AutonomousSourceBuilder + Send>),
    }

    impl From<SendSourceBuilder> for SourceBuilder {
        fn from(value: SendSourceBuilder) -> Self {
            match value {
                SendSourceBuilder::Managed(b) => SourceBuilder::Managed(b),
                SendSourceBuilder::Autonomous(b) => SourceBuilder::Autonomous(b),
            }
        }
    }

    /// Information required to register a new managed source to the measurement pipeline.
    pub struct ManagedSourceRegistration {
        pub name: SourceName,
        pub trigger_spec: trigger::TriggerSpec,
        pub source: Box<dyn source::Source>,
    }

    /// Information required to register a new autonomous source to the measurement pipeline.
    pub struct AutonomousSourceRegistration {
        pub name: SourceName,
        pub source: source::AutonomousSource,
    }

    /// Information required to register a new transform to the measurement pipeline.
    pub struct TransformRegistration {
        pub name: TransformName,
        pub transform: Box<dyn transform::Transform>,
    }

    /// Information required to register a new output to the measurement pipeline.
    pub struct OutputRegistration {
        pub name: OutputName,
        pub output: Box<dyn output::Output>,
    }

    impl std::fmt::Debug for SourceBuilder {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Managed(_) => f.debug_tuple("Managed").field(&"Box<dyn _>").finish(),
                Self::Autonomous(_) => f.debug_tuple("Autonomous").field(&"Box<dyn _>").finish(),
            }
        }
    }

    impl std::fmt::Debug for SendSourceBuilder {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Managed(_) => f.debug_tuple("Managed").field(&"Box<dyn _>").finish(),
                Self::Autonomous(_) => f.debug_tuple("Autonomous").field(&"Box<dyn _>").finish(),
            }
        }
    }
}

/// Contexts used by element builders.
pub mod context {
    use crate::{
        metrics::{Metric, RawMetricId},
        pipeline::{
            registry,
            util::naming::{OutputName, SourceName, TransformName},
        },
    };

    /// Context accessible when building a managed source.
    pub trait SourceBuildContext {
        /// Retrieves a metric by its name.
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;

        /// Generates a name for the source.
        fn source_name(&mut self, name: &str) -> SourceName;
    }

    /// Context accessible when building an autonomous source (not triggered by Alumet).
    pub trait AutonomousSourceBuildContext {
        /// Retrieves a metric by its name.
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        /// Returns a `MetricReader`, which allows to access the metric registry.
        fn metrics_reader(&self) -> registry::MetricReader;
        /// Returns a `MetricSender`, which allows to register new metrics while the pipeline is running.
        fn metrics_sender(&self) -> registry::MetricSender;
        /// Generates a name for the source.
        fn source_name(&mut self, name: &str) -> SourceName;
    }

    /// Context accessible when building a transform.
    pub trait TransformBuildContext {
        /// Retrieves a metric by its name.
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        /// Generates a name for the transform.
        fn transform_name(&mut self, name: &str) -> TransformName;
    }

    /// Context accessible when building an ouptut.
    pub trait OutputBuildContext {
        /// Retrieves a metric by its name.
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        /// Generates a name for the source.
        fn output_name(&mut self, name: &str) -> OutputName;
        /// Returns a handle to the async runtime on which the ouptut will run.
        ///
        /// It can be used to start asynchronous task on the same runtime as the output.
        fn async_runtime(&self) -> &tokio::runtime::Handle;
    }
}

impl Builder {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            trigger_constraints: TriggerConstraints::default(),
            metrics: MetricRegistry::new(),
            metric_listeners: Vec::new(),
            threads_normal: None,
            threads_high_priority: None,
        }
    }

    /// Defines constraints that will be applied, on build, to every registered trigger.
    pub fn set_trigger_constraints(&mut self, constraints: TriggerConstraints) {
        self.trigger_constraints = constraints;
    }

    /// Registers a listener that will be notified of all the new registered metrics,
    /// during build and while the pipeline is running.
    pub fn add_metric_listener(&mut self, listener: registry::MetricListener) {
        self.metric_listeners.push(listener)
    }

    /// Adds a source to the pipeline, with a dedicated builder.
    pub fn add_source_builder(&mut self, plugin: PluginName, builder: elements::SourceBuilder) {
        self.sources.push((plugin, builder))
    }

    /// Adds a transform function to the pipeline, with a dedicated builder.
    pub fn add_transform_builder(&mut self, plugin: PluginName, builder: Box<dyn elements::TransformBuilder>) {
        self.transforms.push((plugin, builder))
    }

    /// Adds an output to the pipeline, with a dedicated builder.
    pub fn add_output_builder(&mut self, plugin: PluginName, builder: Box<dyn elements::OutputBuilder>) {
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
        use context::OutputBuildContext;
        use elements::OutputRegistration;

        fn dummy_output_builder(ctx: &mut dyn OutputBuildContext) -> anyhow::Result<OutputRegistration> {
            use crate::pipeline::{elements::error::WriteError, Output};

            struct DummyOutput;
            impl Output for DummyOutput {
                fn write(&mut self, _m: &MeasurementBuffer, _ctx: &output::OutputContext) -> Result<(), WriteError> {
                    Ok(())
                }
            }

            Ok(OutputRegistration {
                name: ctx.output_name("dummy"),
                output: Box::new(DummyOutput),
            })
        }

        let rt_priority: Option<Runtime> = if self.threads_high_priority == Some(0) {
            None
        } else {
            util::threading::build_priority_runtime(None).ok()
        };
        let rt_normal: Runtime = {
            let n_threads = if let Some(n) = self.threads_normal {
                Some(n)
            } else if rt_priority.is_some() {
                Some(2)
            } else {
                None
            };
            util::threading::build_normal_runtime(n_threads).context("could not build the multithreaded Runtime")?
        };

        // Token to shutdown the pipeline.
        let pipeline_shutdown = CancellationToken::new();

        // Metric registry, global but we can modify it without sending a message
        // thanks to MetricAccess::write().
        let registry_control = MetricRegistryControl::new(self.metrics, self.metric_listeners);
        let (metrics_tx, metrics_rw, metrics_join) =
            registry_control.start(pipeline_shutdown.child_token(), rt_normal.handle());
        let metrics_r = metrics_rw.into_read_only();

        // --- Build the pipeline elements and control loops, with some optimizations ---
        const CHAN_BUF_SIZE: usize = 2048;

        // Channel: sources -> transforms (or sources -> output in case of optimization).
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(CHAN_BUF_SIZE);

        let mut output_control;
        let transform_control;

        if self.outputs.is_empty() {
            log::warn!("No output has been registered. A dummy output will be added to make the pipeline work, but you probably want to add a true output.");
            let no_plugin = PluginName(String::from("_"));
            self.outputs.push((no_plugin, Box::new(dummy_output_builder)));
        }

        if self.outputs.len() == 1 && self.transforms.is_empty() {
            // OPTIMIZATION: there is only one output and no transform,
            // we can connect the inputs directly to the output.
            log::info!("Only one output and no transform, using a simplified and optimized measurement pipeline.");

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(in_rx);
            output_control = output::OutputControl::new(out_rx_provider, rt_normal.handle().clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // No transforms
            transform_control = transform::TransformControl::empty();
        } else {
            // Broadcast queue: transforms -> outputs
            let out_tx = broadcast::Sender::<MeasurementBuffer>::new(CHAN_BUF_SIZE);

            // Outputs
            let out_rx_provider = channel::ReceiverProvider::from(out_tx.clone());
            output_control = output::OutputControl::new(out_rx_provider, rt_normal.handle().clone(), metrics_r.clone());
            output_control
                .blocking_create_outputs(self.outputs)
                .context("output creation failed")?;

            // Transforms
            transform_control = transform::TransformControl::with_transforms(
                self.transforms,
                metrics_r.clone(),
                in_rx,
                out_tx,
                rt_normal.handle(),
            )?;
        };

        // Sources, last in order not to loose any measurement if they start measuring right away.
        let mut source_control = source::SourceControl::new(
            self.trigger_constraints,
            pipeline_shutdown.clone(),
            in_tx,
            rt_normal.handle().clone(),
            rt_priority.as_ref().unwrap_or(&rt_normal).handle().clone(),
            (metrics_r.clone(), metrics_tx.clone()),
        );
        source_control
            .blocking_create_sources(self.sources)
            .context("source creation failed")?;

        // Pipeline control
        let control = PipelineControl::new(source_control, transform_control, output_control);
        let (control_handle, control_join) = control.start(pipeline_shutdown, rt_normal.handle());

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
