use std::{
    collections::HashMap,
    io,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, SystemTime},
};

use crate::{
    metrics::MeasurementBuffer, pipeline::{Output, Source, Transform}, plugin::Plugin
};
use tokio::runtime::Runtime;
use tokio::sync::{broadcast, Mutex as TokioMutex};
use tokio::task::{JoinHandle, JoinSet};

use super::{registry::MetricRegistry, threading};
use tokio_stream::StreamExt;

pub struct TaggedTransform {
    transform: Box<dyn Transform>,
    plugin_name: String,
}
pub struct TaggedOutput {
    output: Box<dyn Output>,
    plugin_name: String,
}
pub struct TaggedSource {
    source: GuardedSource,
    poll_interval: Duration,
    plugin_name: String,
}
enum GuardedSource {
    Normal(Arc<TokioMutex<Box<dyn Source>>>),
    Blocking(Arc<Mutex<Box<dyn Source>>>),
    RealtimePriority(Arc<TokioMutex<Box<dyn Source>>>),
}

impl TaggedSource {
    pub fn new(
        source: Box<dyn Source>,
        source_type: SourceType,
        poll_interval: Duration,
        plugin_name: String,
    ) -> TaggedSource {
        let source = match source_type {
            SourceType::Normal => GuardedSource::Normal(Arc::new(TokioMutex::new(source))),
            SourceType::Blocking => GuardedSource::Blocking(Arc::new(Mutex::new(source))),
            SourceType::RealtimePriority => GuardedSource::RealtimePriority(Arc::new(TokioMutex::new(source))),
        };
        TaggedSource {
            source,
            poll_interval,
            plugin_name,
        }
    }
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceType {
    Normal,
    Blocking,
    RealtimePriority,
}

struct PipelineElements {
    sources: Vec<TaggedSource>,
    transforms: Arc<Mutex<Vec<Box<dyn Transform>>>>,
    outputs: Vec<Arc<Mutex<Box<dyn Output>>>>,
}

struct PipelineParameters {
    normal_worker_threads: Option<usize>,
    priority_worker_threads: Option<usize>,
}

impl PipelineParameters {
    fn build_normal_runtime(&self) -> io::Result<tokio::runtime::Runtime> {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.enable_all().thread_name("normal-worker");
        if let Some(n) = self.normal_worker_threads {
            builder.worker_threads(n);
        }
        builder.build()
    }

    fn build_priority_runtime(&self) -> io::Result<tokio::runtime::Runtime> {
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
        builder.build()
    }
}

/// A builder for measurement pipelines.
pub struct MeasurementPipelineBuilder {
    elements: PipelineElements,
    params: PipelineParameters,
}

impl MeasurementPipelineBuilder {
    pub fn new(sources: Vec<TaggedSource>, transforms: Vec<Box<dyn Transform>>, outputs: Vec<Box<dyn Output>>) -> Self {
        MeasurementPipelineBuilder {
            elements: PipelineElements {
                sources,
                transforms: Arc::new(Mutex::new(transforms)),
                outputs: outputs.into_iter().map(|o| Arc::new(Mutex::new(o))).collect(),
            },
            params: PipelineParameters {
                normal_worker_threads: None,
                priority_worker_threads: None,
            },
        }
    }
    pub fn normal_worker_threads(&mut self, n: usize) {
        self.params.normal_worker_threads = Some(n);
    }
    pub fn priority_worker_threads(&mut self, n: usize) {
        self.params.priority_worker_threads = Some(n);
    }

    /// Creates a new pipeline with the selected parameters, but don't start it yet.
    pub fn build(self) -> io::Result<MeasurementPipeline> {
        // create the runtimes
        let normal_runtime: Runtime = self.params.build_normal_runtime()?;

        let mut priority_runtime: Option<Runtime> = None;
        for src in &self.elements.sources {
            if let GuardedSource::RealtimePriority(_) = src.source {
                priority_runtime = Some(self.params.build_priority_runtime()?);
                break;
            }
        }

        // create the pipeline struct
        Ok(MeasurementPipeline {
            normal_runtime,
            priority_runtime,
            elements: self.elements,
            source_handles_per_plugin: HashMap::new(),
        })
    }
}

/// A pipeline that has been started.
///
/// The pipeline is automatically stopped when dropped.
pub struct MeasurementPipeline {
    // This is necessary to keep the runtimes "alive": runtimes are stopped when dropped.
    normal_runtime: Runtime,
    priority_runtime: Option<Runtime>,

    // We also don't want to lose the pipeline elements when their task finish (we could start a new task).
    // Moreover, the elements are *not* Sync (that would put to much burden on plugin authors), therefore
    // we must ensure that a given element is only being used from one thread at a time.
    // todo: try std::sync::Exclusive instead of Mutex?
    elements: PipelineElements,

    /// Handles to join and abort the tasks
    source_handles_per_plugin: HashMap<String, Vec<JoinHandle<()>>>,
}

impl MeasurementPipeline {
    /// Starts the pipeline and waits for the tasks to finish.
    pub fn run(mut self, metrics: MetricRegistry, on_ready: fn() -> ()) {
        // set the global metric registry, which can be accessed by the pipeline's elements (sources, transforms, outputs)
        MetricRegistry::init_global(metrics);

        // Channel sources -> transforms
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>();

        // if self.elements.transforms.is_empty() && self.elements.outputs.len() == 1 {
            // TODO: If no transforms and one output, the pipeline can be reduced
        // }

        // Broadcast queue transforms -> outputs
        let out_tx = broadcast::Sender::<MeasurementBuffer>::new(256);

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).

        // 1. Outputs (run in parallel, blocking)
        for out in &self.elements.outputs {
            // outputs receive (rx) data from transforms (out_tx)
            let out_rx = out_tx.subscribe();

            // clone the necessary data and spawn the task
            let out = Arc::clone(out);
            self.normal_runtime.spawn_blocking(move || {
                let mut out = out.lock().unwrap();
                run_blocking_output_from_broadcast(out.as_mut(), out_rx);
            });
        }

        // 2. Transforms (run sequentially in one task)
        let transforms = Arc::clone(&self.elements.transforms);
        self.normal_runtime
            .spawn(async move { apply_transforms(transforms, in_rx, out_tx).await });

        // 3. Sources (run in parallel, some blocking, some non-blocking)
        for tagged_src in self.elements.sources {
            // clone the necessary data
            // Note: the timer must be created from the context of a tokio runtime.
            let in_tx = in_tx.clone();
            let plugin_name = tagged_src.plugin_name.clone();
            let poll_interval = tagged_src.poll_interval.clone();

            let handle: JoinHandle<()> = match tagged_src.source {
                GuardedSource::Normal(source) => {
                    let source = source.clone();
                    self.normal_runtime.spawn(async move {
                        let timer = tokio_timerfd::Interval::new_interval(poll_interval).unwrap();
                        let mut source = source.lock().await;
                        poll_source(timer, source.as_mut(), in_tx).await;
                    })
                }
                GuardedSource::Blocking(source) => self.normal_runtime.spawn(async move {
                    let timer = tokio_timerfd::Interval::new_interval(poll_interval).unwrap();
                    poll_blocking_sources(timer, vec![source.clone()], in_tx).await;
                }),
                GuardedSource::RealtimePriority(source) => self
                    .priority_runtime
                    .as_ref()
                    .expect("Some sources require a high-priority runtime, but it was not constructed.")
                    .spawn(async move {
                        let timer = tokio_timerfd::Interval::new_interval(poll_interval).unwrap();
                        let mut source = source.lock().await;
                        poll_source(timer, source.as_mut(), in_tx).await;
                    }),
            };
            self.source_handles_per_plugin
                .entry(plugin_name)
                .or_default()
                .push(handle);
        }

        // The pipeline is now fully started.
        on_ready();

        // Join all source tasks, otherwise the runtime will be dropped and terminate
        self.normal_runtime.block_on(async {
            for (_, tasks) in &mut self.source_handles_per_plugin {
                for t in tasks.iter_mut() {
                    t.await.unwrap_err();
                }
            }
        });
    }
}

async fn poll_blocking_sources(
    mut timer: tokio_timerfd::Interval,
    sources: Vec<Arc<Mutex<Box<dyn Source>>>>,
    tx: mpsc::Sender<MeasurementBuffer>,
) {
    let mut set = JoinSet::new();
    loop {
        // wait for the next tick
        timer.next().await;
        let timestamp = SystemTime::now();

        // spawn one polling task per source, on the "blocking" thread pool
        for src_guard in &sources {
            let src_guard = src_guard.clone();
            let tx = tx.clone();
            set.spawn_blocking(move || {
                // lock the mutex and poll the source
                let mut src = src_guard.lock().unwrap();
                let mut buf = MeasurementBuffer::new(); // todo add size hint
                src.poll(&mut buf.as_accumulator(), timestamp).unwrap();

                // send the results to another task
                tx.send(buf).unwrap();
            });
        }

        // wait for all the tasks to finish
        while let Some(res) = set.join_next().await {
            match res {
                Ok(()) => log::debug!("blocking task finished"),
                Err(err) => log::error!("blocking task failed {}", err),
            }
        }
    }
}

async fn poll_source(mut timer: tokio_timerfd::Interval, src: &mut dyn Source, tx: mpsc::Sender<MeasurementBuffer>) {
    loop {
        // wait for the next tick
        timer.next().await;

        // poll the source
        let mut buf = MeasurementBuffer::new();
        let timestamp = SystemTime::now();
        src.poll(&mut buf.as_accumulator(), timestamp).unwrap();

        // send the results to another task
        tx.send(buf).expect("send failed");
    }
}

async fn poll_sources(
    mut timer: tokio_timerfd::Interval,
    mut sources: Vec<Box<dyn Source>>,
    tx: mpsc::Sender<MeasurementBuffer>,
) {
    loop {
        // wait for the next tick
        timer.next().await;

        // poll the sources
        let mut buf = MeasurementBuffer::new();
        let timestamp = SystemTime::now();

        for src in sources.iter_mut() {
            src.poll(&mut buf.as_accumulator(), timestamp).unwrap();
        }

        // send the results to another task
        tx.send(buf).expect("send failed");
    }
}

async fn apply_transforms(
    transforms: Arc<Mutex<Vec<Box<dyn Transform>>>>,
    rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<MeasurementBuffer>,
) {
    let mut transforms = transforms.lock().unwrap();
    loop {
        // wait for incoming measurements
        if let Ok(mut measurements) = rx.recv() {
            // run the transforms one after another
            for t in transforms.iter_mut() {
                t.apply(&mut measurements).expect("transform failed");
            }
            tx.send(measurements).expect("send failed");
        } else {
            break;
        }
    }
}

fn run_blocking_output_from_broadcast(output: &mut dyn Output, mut rx: broadcast::Receiver<MeasurementBuffer>) {
    loop {
        // wait for incoming measurements
        match rx.blocking_recv() {
            Ok(measurements) => {
                output.write(&measurements).unwrap();
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
            Err(broadcast::error::RecvError::Lagged(n_missed)) => {
                log::warn!("output is too slow, missed {n_missed} entries");
            }
        }
    }
}

fn run_blocking_output_from_channel(mut output: &mut dyn Output, rx: mpsc::Receiver<MeasurementBuffer>) {
    loop {
        // wait for incoming measurements
        match rx.recv() {
            Ok(measurements) => {
                output.write(&measurements).unwrap();
            }
            Err(mpsc::RecvError) => {
                break;
            }
        }
    }
}
