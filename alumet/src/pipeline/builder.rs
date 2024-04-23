use core::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;

use anyhow::Context;

use tokio::runtime::Runtime;
use tokio::sync::{broadcast, mpsc};

use crate::metrics::{Metric, MetricRegistry, RawMetricId};
use crate::{
    measurement::MeasurementBuffer,
    pipeline::{Output, Source, Transform},
};

use super::runtime::{self, IdlePipeline, OutputMsg};
use super::trigger::TriggerSpec;
use super::{threading, SourceType};

/// A builder of measurement pipeline.
pub struct PipelineBuilder {
    pub(crate) sources: Vec<SourceBuilder>,
    pub(crate) transforms: Vec<TransformBuilder>,
    pub(crate) outputs: Vec<OutputBuilder>,
    pub(crate) autonomous_sources: Vec<AutonomousSourceBuilder>,

    pub(crate) metrics: MetricRegistry,

    pub(crate) normal_worker_threads: Option<usize>,
    pub(crate) priority_worker_threads: Option<usize>,
}

pub struct SourceBuilder {
    pub source_type: SourceType,
    pub plugin: String,
    pub build: Box<dyn FnOnce(&PendingPipeline) -> (Box<dyn Source>, TriggerSpec)>,
}

pub struct AutonomousSourceBuilder {
    pub plugin: String,
    pub build: Box<
        dyn FnOnce(
            &PendingPipeline,
            &mpsc::Sender<MeasurementBuffer>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>,
    >,
}

pub struct TransformBuilder {
    pub plugin: String,
    pub build: Box<dyn FnOnce(&PendingPipeline) -> Box<dyn Transform>>,
}

pub struct OutputBuilder {
    pub plugin: String,
    pub build: Box<dyn FnOnce(&PendingPipeline) -> anyhow::Result<Box<dyn Output>>>,
}

/// Information about a pipeline that is being built.
pub struct PendingPipeline<'a> {
    to_output: &'a broadcast::Sender<runtime::OutputMsg>,
    rt_handle: &'a tokio::runtime::Handle,
}

impl<'a> PendingPipeline<'a> {
    pub fn late_registration_handle(&self) -> LateRegistrationHandle {
        let (reply_tx, reply_rx) = mpsc::channel::<Vec<RawMetricId>>(256);
        LateRegistrationHandle {
            to_outputs: self.to_output.clone(),
            reply_tx,
            reply_rx,
        }
    }

    pub fn async_runtime_handle(&self) -> &tokio::runtime::Handle {
        self.rt_handle
    }
}

pub struct LateRegistrationHandle {
    to_outputs: broadcast::Sender<runtime::OutputMsg>,
    reply_tx: mpsc::Sender<Vec<RawMetricId>>,
    reply_rx: mpsc::Receiver<Vec<RawMetricId>>,
}

impl LateRegistrationHandle {
    pub async fn create_metrics_infallible(
        &mut self,
        metrics: Vec<Metric>,
        source_name: String,
    ) -> anyhow::Result<Vec<RawMetricId>> {
        self.to_outputs
            .send(runtime::OutputMsg::RegisterMetrics {
                metrics,
                source_name,
                reply_to: self.reply_tx.clone(),
            })
            .with_context(|| "error on send(OutputMsg::RegisterMetrics)")?;
        match self.reply_rx.recv().await {
            Some(metric_ids) => Ok(metric_ids),
            None => {
                todo!("reply channel closed")
            }
        }
    }
}

/// A source that is ready to run.
pub struct ConfiguredSource {
    /// The source.
    pub source: Box<dyn Source>,
    /// Name of the plugin that registered the source.
    pub plugin_name: String,
    /// Type of the source, for scheduling.
    pub source_type: SourceType,
    /// How to trigger this source.
    pub trigger_provider: TriggerSpec,
}
/// A transform that is ready to run.
pub struct ConfiguredTransform {
    /// The transform.
    pub transform: Box<dyn Transform>,
    /// Name of the plugin that registered the source.
    pub plugin_name: String,
}
/// An output that is ready to run.
pub struct ConfiguredOutput {
    /// The output.
    pub output: Box<dyn Output>,
    /// Name of the plugin that registered the source.
    pub plugin_name: String,
}

#[derive(Debug)]
pub enum PipelineBuildError {
    /// The pipeline's configuration is invalid.
    Invalid(InvalidReason),
    /// Build failure because of an IO error.
    Io(io::Error),
    /// Build failure because a pipeline element (source, transform or output) failed to build
    ElementBuild(anyhow::Error, ElementType, String),
}

#[derive(Debug)]
pub enum ElementType {
    Source,
    Transform,
    Output,
}

#[derive(Debug)]
pub enum InvalidReason {
    NoSource,
    NoOutput,
}

impl fmt::Display for InvalidReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvalidReason::NoSource => write!(f, "no Source"),
            InvalidReason::NoOutput => write!(f, "no Output"),
        }
    }
}

impl fmt::Display for PipelineBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineBuildError::Invalid(reason) => write!(f, "invalid pipeline configuration: {reason}"),
            PipelineBuildError::Io(err) => write!(f, "unable to build the pipeline: {err}"),
            PipelineBuildError::ElementBuild(err, typ, plugin) => write!(
                f,
                "error while building an element of the pipeline ({typ:?} added by plugin {plugin}): {err:?}"
            ),
        }
    }
}

impl From<io::Error> for PipelineBuildError {
    fn from(value: io::Error) -> Self {
        PipelineBuildError::Io(value)
    }
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            autonomous_sources: Vec::new(),
            metrics: MetricRegistry::new(),
            normal_worker_threads: None,
            priority_worker_threads: None,
        }
    }

    pub fn source_count(&self) -> usize {
        self.sources.len() + self.autonomous_sources.len()
    }

    pub fn transform_count(&self) -> usize {
        self.transforms.len()
    }

    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    pub fn metric_iter(&self) -> crate::metrics::MetricIter<'_> {
        self.metrics.iter()
    }

    pub fn build(self) -> Result<IdlePipeline, PipelineBuildError> {
        // Check some conditions.
        if self.metrics.is_empty() {
            log::warn!("No metrics have been registered, have you loaded the right plugins?")
        }
        // The pipeline requires at least 1 source and 1 output, otherwise the channels close (and it would be useless anyway).
        if self.sources.is_empty() && self.autonomous_sources.is_empty() {
            return Err(PipelineBuildError::Invalid(InvalidReason::NoSource));
        }
        if self.outputs.is_empty() {
            return Err(PipelineBuildError::Invalid(InvalidReason::NoSource));
        }

        // Create the runtimes.
        let rt_normal: Runtime = self.build_normal_runtime()?;
        let rt_priority: Option<Runtime> = self.build_priority_runtime()?;

        // Channel: source -> transforms.
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(256);

        // Broadcast queue, used for two things:
        // - transforms -> outputs
        // - late metric registration -> outputs
        let out_tx = broadcast::Sender::<OutputMsg>::new(256);

        // Create the pipeline elements.
        let sources: Vec<ConfiguredSource> = self
            .sources
            .into_iter()
            .map(|builder| {
                let pending = PendingPipeline {
                    to_output: &out_tx,
                    rt_handle: if builder.source_type == SourceType::Normal {
                        rt_normal.handle()
                    } else {
                        rt_priority.as_ref().unwrap().handle()
                    },
                };
                let (source, trigger) = (builder.build)(&pending);
                ConfiguredSource {
                    source,
                    plugin_name: builder.plugin,
                    source_type: builder.source_type,
                    trigger_provider: trigger,
                }
            })
            .collect();

        let pending = PendingPipeline {
            to_output: &out_tx,
            rt_handle: rt_normal.handle(),
        };
        let transforms: Vec<ConfiguredTransform> = self
            .transforms
            .into_iter()
            .map(|builder| {
                let transform = (builder.build)(&pending);
                ConfiguredTransform {
                    transform,
                    plugin_name: builder.plugin,
                }
            })
            .collect();
        let outputs: Result<Vec<ConfiguredOutput>, PipelineBuildError> = self
            .outputs
            .into_iter()
            .map(|builder| {
                let output = (builder.build)(&pending).map_err(|err| {
                    PipelineBuildError::ElementBuild(err, ElementType::Output, builder.plugin.clone())
                })?;
                Ok(ConfiguredOutput {
                    output,
                    plugin_name: builder.plugin,
                })
            })
            .collect();
        let outputs = outputs?;

        // Create the autonomous sources
        let autonomous_sources: Vec<_> = self
            .autonomous_sources
            .into_iter()
            .map(|builder| {
                let data_tx = in_tx.clone();
                let source = (builder.build)(&pending, &data_tx);
                source
            })
            .collect();

        Ok(IdlePipeline {
            sources,
            transforms,
            outputs,
            autonomous_sources,
            metrics: self.metrics,
            from_sources: (in_tx, in_rx),
            to_outputs: out_tx,
            rt_normal,
            rt_priority,
        })
    }

    fn build_normal_runtime(&self) -> io::Result<Runtime> {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.enable_all().thread_name("normal-worker");
        if let Some(n) = self.normal_worker_threads {
            builder.worker_threads(n);
        }
        builder.build()
    }

    fn build_priority_runtime(&self) -> io::Result<Option<Runtime>> {
        if self
            .sources
            .iter()
            .any(|s| s.source_type == SourceType::RealtimePriority)
        {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            builder
                .enable_all()
                .on_thread_start(|| {
                    threading::increase_thread_priority().expect("failed to create high-priority thread for worker")
                })
                .thread_name("priority-worker");
            if let Some(n) = self.priority_worker_threads {
                builder.worker_threads(n);
            }
            Ok(Some(builder.build()?))
        } else {
            Ok(None)
        }
    }
}
