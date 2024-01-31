use std::{
    collections::BTreeMap,
    io,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, SystemTime},
};

use crate::{
    metrics::MeasurementBuffer,
    pipeline::{Output, Source, Transform},
};
use tokio::sync::broadcast;
use tokio::{runtime::Runtime, task::JoinSet};

use super::threading;
use tokio_stream::StreamExt;

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceType {
    Normal,
    Blocking,
    RealtimePriority,
}

pub struct TaggedSource {
    pub source: Box<dyn Source>,
    pub source_type: SourceType,
    pub poll_interval: Duration,
}

pub struct MeasurementPipeline {
    elements: PipelineElements,
    params: PipelineParameters,
}

struct PipelineElements {
    sources: Vec<TaggedSource>,
    transforms: Vec<Box<dyn Transform>>,
    outputs: Vec<Box<dyn Output>>,
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

/// A pipeline that has been started.
/// 
/// The pipeline is automatically stopped when dropped.
pub struct RunningPipeline {
    // This is necessary to keep the runtimes "alive": runtimes are stopped when dropped.
    normal_runtime: Runtime,
    priority_runtime: Option<Runtime>
}

impl MeasurementPipeline {
    pub fn new(sources: Vec<TaggedSource>, transforms: Vec<Box<dyn Transform>>, outputs: Vec<Box<dyn Output>>) -> Self {
        MeasurementPipeline {
            elements: PipelineElements {
                sources,
                transforms,
                outputs,
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

    pub fn start(self) -> RunningPipeline {
        let params = self.params;
        let elems = self.elements;

        // group sources by type and polling interval (this may change in the future)
        let sources = group_sources(elems.sources);

        // Build the normal runtime now but the priority runtime on demand
        let normal_runtime: Runtime = params
            .build_normal_runtime()
            .expect("A tokio runtime is required for the pipeline, but couldn't be started");
        let mut priority_runtime: Option<Runtime> = None;

        // Channel sources -> transforms
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>();

        if elems.transforms.is_empty() && elems.outputs.len() == 1 {
            // TODO: If no transforms and one output, the pipeline can be reduced
        }

        // Broadcast queue transforms -> outputs
        let out_tx = broadcast::Sender::<MeasurementBuffer>::new(256);

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).
        // Outputs (run in parallel, blocking)
        for out in elems.outputs {
            let out_rx = out_tx.subscribe();
            normal_runtime.spawn_blocking(move || {
                run_blocking_output_from_broadcast(out, out_rx);
            });
        }

        // Transforms (run sequentially)
        normal_runtime.spawn(apply_transforms(elems.transforms, in_rx, out_tx));

        // Sources (run in parallel, some blocking, some non-blocking)
        for ((typ, poll_interval), grouped_sources) in sources {
            let in_tx = in_tx.clone();
            let timer = tokio_timerfd::Interval::new_interval(poll_interval).unwrap();
            match typ {
                SourceType::Normal => {
                    normal_runtime.spawn(poll_sources(timer, grouped_sources, in_tx));
                }
                SourceType::Blocking => {
                    let guarded_sources = grouped_sources.into_iter().map(|s| Arc::new(Mutex::new(s))).collect();
                    normal_runtime.spawn(poll_blocking_sources(timer, guarded_sources, in_tx));
                }
                SourceType::RealtimePriority => {
                    priority_runtime
                        .get_or_insert_with(|| {
                            params.build_priority_runtime().expect(
                                "Some sources require a high-priority mode, but the tokio runtime failed to start",
                            )
                        })
                        .spawn(poll_sources(timer, grouped_sources, in_tx));
                }
            }
        }
        
        // prevent the runtimes from being dropped (that would stop them) 
        RunningPipeline {
            normal_runtime,
            priority_runtime,
        }
    }
}

fn group_sources(sources: Vec<TaggedSource>) -> BTreeMap<(SourceType, Duration), Vec<Box<dyn Source>>> {
    let mut result: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for s in sources {
        result
            .entry((s.source_type, s.poll_interval))
            .or_default()
            .push(s.source);
    }
    result
}

pub async fn poll_blocking_sources(
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

pub async fn poll_sources(
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

pub async fn apply_transforms(
    mut transforms: Vec<Box<dyn Transform>>,
    rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<MeasurementBuffer>,
) {
    loop {
        // wait for incoming measurements
        if let Ok(mut measurements) = rx.recv() {
            // run the transforms one after another
            for t in &mut transforms {
                t.apply(&mut measurements).expect("transform failed");
            }
            tx.send(measurements).expect("send failed");
        } else {
            break;
        }
    }
}

pub fn run_blocking_output_from_broadcast(mut output: Box<dyn Output>, mut rx: broadcast::Receiver<MeasurementBuffer>) {
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

pub fn run_blocking_output_from_channel(mut output: Box<dyn Output>, rx: mpsc::Receiver<MeasurementBuffer>) {
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
