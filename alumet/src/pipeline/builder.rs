use super::elements::{output, source, transform};
use super::registry::{MetricReader, MetricSender};
use crate::pipeline::registry::MetricRegistryControl;
use crate::{measurement::MeasurementBuffer, metrics::MetricRegistry};

use super::util::naming::PluginName;
use super::{
    control::{ControlHandle, PipelineControl},
    trigger::TriggerConstraints,
    util,
};
use anyhow::Context;
use tokio::{
    runtime::Runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub struct MeasurementPipeline {
    rt_normal: Runtime,
    rt_priority: Option<Runtime>,
    control_handle: ControlHandle,
    metrics: (MetricSender, MetricReader),
    pipeline_control_task: JoinHandle<()>,
    metrics_control_task: JoinHandle<()>,
}

/// Constructs measurement pipelines.
pub struct Builder {
    sources: Vec<(PluginName, elements::SourceBuilder)>,
    transforms: Vec<(PluginName, Box<dyn elements::TransformBuilder>)>,
    outputs: Vec<(PluginName, Box<dyn elements::OutputBuilder>)>,

    /// Constraints to apply to the TriggerSpec of managed sources.
    trigger_constraints: TriggerConstraints,

    /// Metrics
    pub metrics: MetricRegistry,
}

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
    pub trait ManagedSourceBuilder: FnOnce(&mut dyn SourceBuildContext) -> ManagedSourceRegistration {}
    impl<F> ManagedSourceBuilder for F where F: FnOnce(&mut dyn SourceBuildContext) -> ManagedSourceRegistration {}

    pub trait AutonomousSourceBuilder:
        FnOnce(
        &mut dyn SourceBuildContext,
        CancellationToken,
        Sender<MeasurementBuffer>,
    ) -> AutonomousSourceRegistration
    {
    }
    impl<F> AutonomousSourceBuilder for F where
        F: FnOnce(
            &mut dyn SourceBuildContext,
            CancellationToken,
            Sender<MeasurementBuffer>,
        ) -> AutonomousSourceRegistration
    {
    }

    pub trait TransformBuilder: FnOnce(&mut dyn TransformBuildContext) -> TransformRegistration {}
    impl<F> TransformBuilder for F where F: FnOnce(&mut dyn TransformBuildContext) -> TransformRegistration {}

    pub trait OutputBuilder: FnOnce(&mut dyn OutputBuildContext) -> OutputRegistration {}
    impl<F> OutputBuilder for F where F: FnOnce(&mut dyn OutputBuildContext) -> OutputRegistration {}

    pub(crate) enum SourceBuilder {
        Managed(Box<dyn ManagedSourceBuilder>),
        Autonomous(Box<dyn AutonomousSourceBuilder>),
    }

    pub(crate) enum SendSourceBuilder {
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

    pub struct ManagedSourceRegistration {
        pub name: SourceName,
        pub trigger: trigger::TriggerSpec,
        pub source: Box<dyn source::Source>,
    }

    pub struct AutonomousSourceRegistration {
        pub name: SourceName,
        pub source: source::AutonomousSource,
    }

    pub struct TransformRegistration {
        pub name: TransformName,
        pub transform: Box<dyn transform::Transform>,
    }

    pub struct OutputRegistration {
        pub name: OutputName,
        pub output: Box<dyn output::Output>,
    }
}

pub mod context {
    use crate::{
        metrics::{Metric, MetricRegistry, RawMetricId},
        pipeline::util::naming::{OutputName, ScopedNameGenerator, SourceName, TransformName},
    };

    pub trait SourceBuildContext {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        fn source_name(&mut self, name: &str) -> SourceName;
    }

    pub trait TransformBuildContext {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        fn transform_name(&mut self, name: &str) -> TransformName;
    }

    pub trait OutputBuildContext {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        fn output_name(&mut self, name: &str) -> OutputName;
    }

    pub(crate) struct BuilderContext<'a> {
        metrics: &'a MetricRegistry,
        namegen: &'a mut ScopedNameGenerator,
    }

    impl<'a> BuilderContext<'a> {
        pub(crate) fn new(metrics: &'a MetricRegistry, namegen: &'a mut ScopedNameGenerator) -> Self {
            Self { metrics, namegen }
        }
    }

    impl SourceBuildContext for BuilderContext<'_> {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
            self.metrics.by_name(name)
        }

        fn source_name(&mut self, name: &str) -> SourceName {
            self.namegen.source_name(name)
        }
    }

    impl TransformBuildContext for BuilderContext<'_> {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
            SourceBuildContext::metric_by_name(self, name)
        }

        fn transform_name(&mut self, name: &str) -> TransformName {
            self.namegen.transform_name(name)
        }
    }

    impl OutputBuildContext for BuilderContext<'_> {
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
            SourceBuildContext::metric_by_name(self, name)
        }

        fn output_name(&mut self, name: &str) -> TransformName {
            self.namegen.output_name(name)
        }
    }
}

impl Builder {
    pub fn new(trigger_constraints: TriggerConstraints) -> Self {
        Self {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            trigger_constraints,
            metrics: MetricRegistry::new(),
        }
    }

    pub fn add_source_builder(&mut self, plugin: PluginName, builder: elements::SourceBuilder) {
        self.sources.push((plugin, builder))
    }

    pub fn add_transform_builder(&mut self, plugin: PluginName, builder: Box<dyn elements::TransformBuilder>) {
        self.transforms.push((plugin, builder))
    }

    pub fn add_output_builder(&mut self, plugin: PluginName, builder: Box<dyn elements::OutputBuilder>) {
        self.outputs.push((plugin, builder))
    }

    /// Builds the measurement pipeline.
    ///
    /// The new pipeline is immediately started.
    pub fn build(self) -> anyhow::Result<MeasurementPipeline> {
        let rt_priority: Option<Runtime> = util::threading::build_priority_runtime(None).ok();
        let rt_normal: Runtime = {
            let normal_workers = if rt_priority.is_some() { Some(2) } else { None };
            util::threading::build_normal_runtime(normal_workers)
                .context("could not build the multithreaded Runtime")?
        };

        // Channel: sources -> transforms.
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(256);

        // Broadcast queue: transforms -> outputs
        let out_tx = broadcast::Sender::<MeasurementBuffer>::new(256);

        // Token to shutdown the pipeline.
        let pipeline_shutdown = CancellationToken::new();

        // Metric registry, global but we can modify it without sending a message
        // thanks to MetricAccess::write().
        let registry_control = MetricRegistryControl::new(self.metrics);
        let (metrics_tx, metrics_rw, metrics_join) =
            registry_control.start(pipeline_shutdown.child_token(), rt_normal.handle());
        let metrics_r = metrics_rw.into_read_only();

        // Create pipeline elements, sources last in order not to loose
        // any measurement if they start polling right away.

        // Outputs
        let mut output_control =
            output::OutputControl::new(out_tx.clone(), rt_normal.handle().clone(), metrics_r.clone());
        output_control.create_outputs(self.outputs);

        // Transforms
        let transform_control = transform::TransformControl::with_transforms(
            self.transforms,
            metrics_r.clone(),
            in_rx,
            out_tx,
            rt_normal.handle(),
        );

        // Sources
        let mut source_control = source::SourceControl::new(
            self.trigger_constraints,
            pipeline_shutdown.clone(),
            in_tx,
            rt_normal.handle().clone(),
            rt_normal.handle().clone(),
            metrics_r.clone(),
        );
        source_control.create_sources(self.sources);

        // Pipeline control
        let control = PipelineControl::new(source_control, transform_control, output_control);
        let (control_handle, control_join) = control.start(pipeline_shutdown, rt_normal.handle());

        // Done!
        Ok(MeasurementPipeline {
            rt_normal,
            rt_priority,
            control_handle,
            metrics: (metrics_tx, metrics_r),
            pipeline_control_task: control_join,
            metrics_control_task: metrics_join,
        })
    }

    pub fn stats(&self) -> BuilderStats {
        BuilderStats {
            sources: self.sources.len(),
            transforms: self.transforms.len(),
            outputs: self.outputs.len(),
            metrics: self.metrics.len(),
        }
    }
}

pub struct BuilderStats {
    pub sources: usize,
    pub transforms: usize,
    pub outputs: usize,
    pub metrics: usize,
}

impl MeasurementPipeline {
    pub fn control_handle(&self, plugin: PluginName) -> ControlHandle {
        self.control_handle.clone_with_plugin(plugin)
    }

    pub fn metrics_sender(&self) -> MetricSender {
        self.metrics.0.clone()
    }

    pub fn async_runtime(&self) -> &tokio::runtime::Handle {
        self.rt_normal.handle()
    }

    pub async fn wait_for_shutdown(self) -> Result<(), tokio::task::JoinError> {
        self.pipeline_control_task.await?;
        self.metrics_control_task.await?;
        Ok(())
    }

    pub fn blocking_wait_for_shutdown(self) -> Result<(), tokio::task::JoinError> {
        let rt = self.async_runtime().clone();
        rt.block_on(self.wait_for_shutdown())
    }
}
