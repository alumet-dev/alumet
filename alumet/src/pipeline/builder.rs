use core::fmt;
use std::collections::HashMap;
use std::future::Future;
use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use anyhow::Context;

use tokio::runtime::Runtime;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::metrics::{Metric, MetricRegistry, RawMetricId};
use crate::{
    measurement::MeasurementBuffer,
    pipeline::{Output, Source, Transform},
};

use super::runtime::{self, IdlePipeline, OutputMsg};
use super::trigger::{TriggerConstraints, TriggerSpec};

/// A builder of measurement pipeline.
pub struct PipelineBuilder {
    pub(crate) namegen: ElementNameGenerator,
    pub(crate) sources: Vec<ManagedSourceBuilder>,
    pub(crate) transforms: Vec<TransformBuilder>,
    pub(crate) outputs: Vec<OutputBuilder>,
    pub(crate) autonomous_sources: Vec<AutonomousSourceBuilder>,

    pub(crate) source_constraints: TriggerConstraints,

    pub(crate) metrics: MetricRegistry,
    pub(crate) allow_no_metrics: bool,

    pub(crate) normal_worker_threads: Option<usize>,
    pub(crate) priority_worker_threads: Option<usize>,
}

pub type SourceBuildFn = dyn FnOnce(&PendingPipelineContext) -> Box<dyn Source>;
pub type AutonomousSourceBuildFn = dyn FnOnce(
    &PendingPipelineContext,
    CancellationToken,
    mpsc::Sender<MeasurementBuffer>,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

pub struct ManagedSourceBuilder {
    pub name: String,
    pub plugin: String,
    pub trigger: TriggerSpec,
    pub build: Box<SourceBuildFn>,
}

pub struct AutonomousSourceBuilder {
    pub name: String,
    pub plugin: String,
    pub build: Box<AutonomousSourceBuildFn>,
}

pub struct TransformBuilder {
    pub name: String,
    pub plugin: String,
    pub build: Box<dyn FnOnce(&PendingPipelineContext) -> Box<dyn Transform>>,
}

pub struct OutputBuilder {
    pub name: String,
    pub plugin: String,
    pub build: Box<dyn FnOnce(&PendingPipelineContext) -> anyhow::Result<Box<dyn Output>>>,
}

/// Information about a pipeline that is being built.
pub struct PendingPipelineContext<'a> {
    to_output: &'a broadcast::Sender<runtime::OutputMsg>,
    rt_handle: &'a tokio::runtime::Handle,
}

impl<'a> PendingPipelineContext<'a> {
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
pub(super) struct ConfiguredSource {
    /// The source.
    pub source: Box<dyn Source>,
    /// Name of the source.
    pub name: String,
    /// Name of the plugin that registered the source.
    pub plugin_name: String,
    /// How to trigger this source.
    pub trigger_provider: TriggerSpec,
}
/// An autonomous source that is ready to run.
pub(super) struct ConfiguredAutonomousSource {
    /// The source.
    pub source: Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>,
    /// Name of the source.
    pub name: String,
}
/// A transform that is ready to run.
pub(super) struct ConfiguredTransform {
    /// The transform.
    pub transform: Box<dyn Transform>,
    /// Name of the transform.
    pub name: String,
    /// Name of the plugin that registered the source.
    pub plugin_name: String,
}
/// An output that is ready to run.
pub(super) struct ConfiguredOutput {
    /// The output.
    pub output: Box<dyn Output>,
    /// Name of the output.
    pub name: String,
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

impl std::error::Error for PipelineBuildError {}

impl fmt::Display for PipelineBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineBuildError::Invalid(reason) => write!(f, "invalid pipeline configuration: {reason}"),
            PipelineBuildError::Io(err) => write!(f, "IO error while building the pipeline: {err}"),
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
            namegen: ElementNameGenerator::new(),
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            autonomous_sources: Vec::new(),
            metrics: MetricRegistry::new(),
            allow_no_metrics: false,
            normal_worker_threads: None,
            priority_worker_threads: None,
            source_constraints: TriggerConstraints::default(),
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
        if self.metrics.is_empty() && !self.allow_no_metrics {
            log::warn!("No metrics have been registered, have you loaded the right plugins?")
        }
        // The pipeline requires at least 1 source and 1 output, otherwise the channels close (and it would be useless anyway).
        if self.sources.is_empty() && self.autonomous_sources.is_empty() {
            return Err(PipelineBuildError::Invalid(InvalidReason::NoSource));
        }
        if self.outputs.is_empty() {
            return Err(PipelineBuildError::Invalid(InvalidReason::NoSource));
        }

        // Create the normal runtime, the priority one is initialized on demand.
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
                let name = builder.name;
                let mut trigger = builder.trigger;
                let pending = PendingPipelineContext {
                    to_output: &out_tx,
                    rt_handle: if trigger.realtime_priority {
                        rt_priority
                            .as_ref()
                            .unwrap_or_else(|| {
                                log::warn!("Could not provide a \"realtime priority\" runtime for source {name}, using the normal runtime (see previous warnings).");
                                &rt_normal
                            })
                            .handle()
                    } else {
                        rt_normal.handle()
                    },
                };
                let source = (builder.build)(&pending);
                log::trace!("(source {name}) TriggerSpec before constraints: {trigger:?}",);
                trigger.constrain(&self.source_constraints);
                log::trace!("(source {name}) TriggerSpec after constraints: {trigger:?}",);

                ConfiguredSource {
                    source,
                    name,
                    plugin_name: builder.plugin,
                    trigger_provider: trigger,
                }
            })
            .collect();

        let pending = PendingPipelineContext {
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
                    name: builder.name,
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
                    name: builder.name,
                    plugin_name: builder.plugin,
                })
            })
            .collect();
        let outputs = outputs?;

        // Create the autonomous sources
        let autonomous_shutdown_token = CancellationToken::new();
        let autonomous_sources: Vec<_> = self
            .autonomous_sources
            .into_iter()
            .map(|builder| {
                let data_tx = in_tx.clone();
                let name = builder.name;
                // This token will be cancelled when the global token gets cancelled (Alumet is shutting down).
                // It can also be cancelled on its own, in which case only this source will be stopped.
                let cancel_token = autonomous_shutdown_token.child_token();
                let source = (builder.build)(&pending, cancel_token, data_tx);
                ConfiguredAutonomousSource { source, name }
            })
            .collect();

        Ok(IdlePipeline {
            sources,
            transforms,
            outputs,
            autonomous_sources,
            autonomous_shutdown_token,
            metrics: self.metrics,
            from_sources: (in_tx, in_rx),
            to_outputs: out_tx,
            rt_normal,
            rt_priority,
        })
    }

    fn build_normal_runtime(&self) -> io::Result<Runtime> {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.enable_all().thread_name_fn(|| {
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("normal-worker-{id}")
        });
        if let Some(n) = self.normal_worker_threads {
            builder.worker_threads(n);
        }
        builder.build()
    }

    fn build_priority_runtime(&self) -> io::Result<Option<Runtime>> {
        fn resolve_application_path() -> io::Result<PathBuf> {
            std::env::current_exe()?.canonicalize()
        }

        // Count how many sources require a "realtime priority" runtime
        let n_rt_sources = self
            .sources
            .iter()
            .filter(|builder| builder.trigger.realtime_priority)
            .count();

        if n_rt_sources > 0 {
            // If `on_thread_start` fails, `builder.build()` will still return a runtime,
            // but it will be unusable. To avoid that, we store the error here and don't return Some(runtime).
            static THREAD_START_FAILURE: Mutex<Option<io::Error>> = Mutex::new(None);

            let mut builder = tokio::runtime::Builder::new_multi_thread();
            builder
                .enable_all()
                .worker_threads(n_rt_sources)
                .on_thread_start(|| {
                    if let Err(e) = super::threading::increase_thread_priority() {
                        let mut failure = THREAD_START_FAILURE.lock().unwrap();
                        if failure.is_none() {
                            let hint =
                                if e.kind() == ErrorKind::PermissionDenied {
                                    let app_path = resolve_application_path()
                                        .ok()
                                        .and_then(|p| p.to_str().map(|s| s.to_owned()))
                                        .unwrap_or(String::from("path/to/agent"));

                                    indoc::formatdoc! {"
                                        This is probably caused by insufficient privileges.
                                        
                                        To fix this, you have two possibilities:
                                        1. Grant the SYS_NICE capability to the agent binary.
                                             sudo setcap cap_sys_nice+ep \"{app_path}\"
                                        
                                           Note: to grant multiple capabilities to the binary, you must put all the capabilities in the same command.
                                             sudo setcap \"cap_sys_nice+ep cap_perfmon=ep\" \"{app_path}\"
                                        
                                        2. Run the agent as root (not recommended).
                                    "}
                                } else {
                                    String::from("This does not seem to be caused by insufficient privileges. Please report an issue on the GitHub repository.")
                                };
                            log::error!("I tried to increase the scheduling priority of the thread in order to improve the accuracy of the measurement timing, but I failed: {e}\n{hint}");
                            log::warn!("Alumet will still work, but the time between two measurements may differ from the configuration.");
                            *failure = Some(e);
                        }
                        let current_thread = std::thread::current();
                        let thread_name = current_thread.name().unwrap_or("<unnamed>");
                        log::warn!("Unable to increase the scheduling priority of thread {thread_name}.");
                    };
                })
                .thread_name_fn(|| {
                    static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                    let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                    format!("priority-worker-{id}")
                });
            let n_threads = self.priority_worker_threads.unwrap_or(n_rt_sources);
            builder.worker_threads(n_threads);

            // Build the runtime.
            let runtime = builder.build()?;

            // Try to spawn a task to ensure that the worker threads have started properly.
            // Otherwise, builder.build() may return and the threads may fail after the failure check.
            runtime.block_on(async {
                let _ = runtime
                    .spawn(tokio::time::sleep(tokio::time::Duration::from_millis(1)))
                    .await;
            });

            // If the worker threads failed to start, don't use this runtime.
            if THREAD_START_FAILURE.lock().unwrap().take().is_some() {
                return Ok(None);
            }
            Ok(Some(runtime))
        } else {
            Ok(None)
        }
    }
}

/// Generates names for the pipeline elements.
pub(crate) struct ElementNameGenerator {
    existing_names: HashMap<String, usize>,
}

impl ElementNameGenerator {
    pub fn new() -> Self {
        Self {
            existing_names: HashMap::new(),
        }
    }

    pub fn deduplicate(&mut self, mut name: String, always_suffix: bool) -> String {
        use std::fmt::Write;

        match self.existing_names.get_mut(&name) {
            Some(n) => {
                *n += 1;
                write!(name, "-{n}").unwrap();
            }
            None => {
                self.existing_names.insert(name.clone(), 0);
                if always_suffix {
                    write!(name, "-0").unwrap();
                }
            }
        }
        name
    }
}

impl Default for ElementNameGenerator {
    fn default() -> Self {
        Self::new()
    }
}
