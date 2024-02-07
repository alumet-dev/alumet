use core::fmt;
use std::{
    collections::HashMap, future::Future, io, ops::BitOrAssign, pin::Pin, sync::{atomic::{AtomicU64, AtomicUsize, Ordering}, Arc}, time::{Duration, Instant, SystemTime}
};

use crate::{
    metrics::MeasurementBuffer,
    pipeline::{Output, Source, Transform},
};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::{runtime::Runtime, sync::watch};

use super::registry::MetricRegistry;
use super::{
    threading, PollError, PollErrorKind, TransformError, TransformErrorKind, WriteError,
};
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
    source: Box<dyn Source>,
    source_type: SourceType,
    trigger_provider: SourceTriggerProvider,
    plugin_name: String,
}
/// A boxed future, from the `futures` crate.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug)]
pub enum SourceTriggerProvider {
    TimeInterval {
        start_time: Instant,
        poll_interval: Duration,
        flush_interval: Duration,
    },
    Future {
        f: fn() -> BoxFuture<'static, SourceTriggerOutput>,
        flush_rounds: usize,
    },
}
impl SourceTriggerProvider {
    pub fn provide(self) -> io::Result<(SourceTrigger, usize)> {
        match self {
            SourceTriggerProvider::TimeInterval {
                start_time,
                poll_interval,
                flush_interval,
            } => {
                let flush_rounds = (flush_interval.as_micros() / poll_interval.as_micros()) as usize;
                let trigger = SourceTrigger::TimeInterval(tokio_timerfd::Interval::new(start_time, poll_interval)?);
                Ok((trigger, flush_rounds))
            }
            SourceTriggerProvider::Future { f, flush_rounds } => {
                let trigger = SourceTrigger::Future(f);
                Ok((trigger, flush_rounds))
            }
        }
    }
}

pub type SourceTriggerOutput = Result<(), PollError>;
pub enum SourceTrigger {
    TimeInterval(tokio_timerfd::Interval),
    Future(fn() -> BoxFuture<'static, SourceTriggerOutput>),
}
impl fmt::Debug for SourceTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimeInterval(_) => f.write_str("TimeInterval"),
            Self::Future(_) => f.write_str("Future"),
        }
    }
}

impl TaggedSource {
    pub fn new(
        source: Box<dyn Source>,
        source_type: SourceType,
        trigger_provider: SourceTriggerProvider,
        plugin_name: String,
    ) -> TaggedSource {
        TaggedSource {
            source,
            source_type,
            trigger_provider,
            plugin_name,
        }
    }
}
impl TaggedOutput {
    pub fn new(plugin_name: String, output: Box<dyn Output>) -> TaggedOutput {
        TaggedOutput { output, plugin_name }
    }
}
impl TaggedTransform {
    pub fn new(plugin_name: String, transform: Box<dyn Transform>) -> TaggedTransform {
        TaggedTransform { transform, plugin_name }
    }
}
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceType {
    Normal,
    // Blocking, // todo: how to provide this type properly?
    RealtimePriority,
}

struct PipelineElements {
    sources: Vec<TaggedSource>,
    transforms: Vec<TaggedTransform>,
    outputs: Vec<TaggedOutput>,
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
pub struct PendingPipeline {
    elements: PipelineElements,
    params: PipelineParameters,
}

pub struct PipelineController {
    // Keep the tokio runtimes alive
    normal_runtime: Runtime,
    _priority_runtime: Option<Runtime>,

    // Handles to wait for sources to finish.
    source_handles: Vec<JoinHandle<Result<(), PollError>>>,
    output_handles: Vec<JoinHandle<Result<(), WriteError>>>,
    transform_handle: JoinHandle<Result<(), TransformError>>,

    // Senders to keep the receivers alive and to send commands.
    source_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<SourceCmd>>>,
    output_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<OutputCmd>>>,
    
    /// Currently active transforms.
    /// Note: it could be generalized to support more than 64 values,
    /// either with a crate like arc-swap, or by using multiple Vec of transforms, each with an AtomicU64. 
    active_transforms: Arc<AtomicU64>,
    transforms_mask_by_plugin: HashMap<String, u64>,
}
impl PipelineController {
    /// Blocks the current thread until all tasks in the pipeline finish.
    pub fn wait_for_all(&mut self) {
        self.normal_runtime.block_on(async {
            for handle in &mut self.source_handles {
                handle.await.unwrap().unwrap();// todo: handle errors
            }

            (&mut self.transform_handle).await.unwrap().unwrap();

            for handle in &mut self.output_handles {
                handle.await.unwrap().unwrap();
            }
        });
    }

    pub fn command_plugin_transforms(&self, plugin_name: &str, command: TransformCmd) {
        let mask: u64 = *self.transforms_mask_by_plugin.get(plugin_name).unwrap();
        match command {
            TransformCmd::Enable => self.active_transforms.fetch_or(mask, Ordering::Relaxed),
            TransformCmd::Disable => self.active_transforms.fetch_nand(mask, Ordering::Relaxed),
        };
    }

    pub fn command_plugin_sources(&self, plugin_name: &str, command: SourceCmd) {
        let senders = self.source_command_senders_by_plugin.get(plugin_name).unwrap();
        for s in senders {
            s.send(command.clone()).unwrap();
        }
    }

    pub fn command_all_sources(&self, command: SourceCmd) {
        for (_, senders) in &self.source_command_senders_by_plugin {
            for s in senders {
                s.send(command.clone()).unwrap();
            }
        }
    }

    pub fn command_plugin_outputs(&self, plugin_name: &str, command: OutputCmd) {
        let senders = self.output_command_senders_by_plugin.get(plugin_name).unwrap();
        for s in senders {
            s.send(command.clone()).unwrap();
        }
    }

    pub fn command_all_outputs(&self, command: OutputCmd) {
        for (_, senders) in &self.output_command_senders_by_plugin {
            for s in senders {
                s.send(command.clone()).unwrap();
            }
        }
    }
}

impl PendingPipeline {
    pub fn new(sources: Vec<TaggedSource>, transforms: Vec<TaggedTransform>, outputs: Vec<TaggedOutput>) -> Self {
        PendingPipeline {
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

    pub fn start(self, metrics: MetricRegistry) -> PipelineController {
        // set the global metric registry, which can be accessed by the pipeline's elements (sources, transforms, outputs)
        MetricRegistry::init_global(metrics);

        // Create the runtimes
        let normal_runtime: Runtime = self.params.build_normal_runtime().unwrap();

        let priority_runtime: Option<Runtime> = {
            let mut res = None;
            for src in &self.elements.sources {
                if src.source_type == SourceType::RealtimePriority {
                    res = Some(self.params.build_priority_runtime().unwrap());
                    break;
                }
            }
            res
        };

        // Channel sources -> transforms
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(256);

        // if self.elements.transforms.is_empty() && self.elements.outputs.len() == 1 {
        // TODO: If no transforms and one output, the pipeline can be reduced
        // }

        // Broadcast queue transforms -> outputs
        let out_tx = broadcast::Sender::<MeasurementBuffer>::new(256);

        // Store the task handles in order to wait for them to complete before stopping,
        // and the command senders in order to keep the receivers alive and to be able to send commands after the launch.
        let mut source_handles = Vec::with_capacity(self.elements.sources.len());
        let mut output_handles = Vec::with_capacity(self.elements.outputs.len());
        let mut source_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut output_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut transforms_indexes_by_plugin: HashMap<_, u64> = HashMap::new();

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).
        // 1. Outputs
        for out in self.elements.outputs {
            let data_rx = out_tx.subscribe();
            let (command_tx, command_rx) = watch::channel(OutputCmd::Run);
            let handle = normal_runtime.spawn(run_output_from_broadcast(out.output, data_rx, command_rx));
            output_handles.push(handle);
            output_command_senders_by_plugin.entry(out.plugin_name).or_default().push(command_tx);
        }

        // 2. Transforms
        let active_transforms = Arc::new(AtomicU64::new(u64::MAX)); // all active by default
        let mut transforms = Vec::with_capacity(self.elements.transforms.len());
        for (i, t) in self.elements.transforms.into_iter().enumerate() {
            transforms.push(t.transform);
            let mask: u64 = 1 << i;
            transforms_indexes_by_plugin.entry(t.plugin_name).or_default().bitor_assign(mask);
        }
        let transform_handle = normal_runtime.spawn(run_transforms(transforms, in_rx, out_tx, active_transforms.clone()));

        // 3. Sources
        for src in self.elements.sources {
            let data_tx = in_tx.clone();
            let (command_tx, command_rx) = watch::channel(SourceCmd::SetTrigger(Some(src.trigger_provider)));
            let runtime = match src.source_type {
                SourceType::Normal => &normal_runtime,
                SourceType::RealtimePriority => priority_runtime.as_ref().unwrap(),
            };
            let handle = runtime.spawn(run_source(src.source, data_tx, command_rx));
            source_handles.push(handle);
            source_command_senders_by_plugin.entry(src.plugin_name).or_default().push(command_tx);
        }

        PipelineController {
            normal_runtime,
            _priority_runtime: priority_runtime,
            source_handles,
            output_handles,
            transform_handle,
            source_command_senders_by_plugin,
            output_command_senders_by_plugin,
            active_transforms,
            transforms_mask_by_plugin: transforms_indexes_by_plugin,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SourceCmd {
    Run,
    Pause,
    Stop,
    SetTrigger(Option<SourceTriggerProvider>),
}

async fn run_source(
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    mut commands: watch::Receiver<SourceCmd>,
) -> Result<(), PollError> {
    fn init_trigger(provider: &mut Option<SourceTriggerProvider>) -> Result<(SourceTrigger, usize), PollError> {
        provider
            .take()
            .expect("invalid empty trigger in message Init(trigger)")
            .provide()
            .map_err(|e| {
                PollError::with_source(PollErrorKind::Unrecoverable, "Source trigger initialization failed", e)
            })
    }

    // the first command must be "init"
    let (mut trigger, mut flush_rounds) = {
        let init_cmd = commands
            .wait_for(|c| matches!(c, SourceCmd::SetTrigger(_)))
            .await
            .map_err(|e| {
                PollError::with_source(PollErrorKind::Unrecoverable, "Source task initialization failed", e)
            })?;

        match (*init_cmd).clone() {
            // cloning required to borrow opt as mut below
            SourceCmd::SetTrigger(mut opt) => init_trigger(&mut opt)?,
            _ => unreachable!(),
        }
    };

    // Stores measurements in this buffer, and replace it every `flush_rounds` rounds.
    // We probably need the capacity to store at least one measurement per round.
    let mut buffer = MeasurementBuffer::with_capacity(flush_rounds);

    // main loop
    let mut i = 1usize;
    'run: loop {
        // wait for trigger
        match trigger {
            SourceTrigger::TimeInterval(ref mut interval) => {
                interval.next().await.unwrap().unwrap();
            }
            SourceTrigger::Future(f) => {
                f().await?;
            }
        };

        // poll the source
        let timestamp = SystemTime::now();
        source.poll(&mut buffer.as_accumulator(), timestamp)?;

        // Flush the measurements and update the command, not on every round for performance reasons.
        // This is done _after_ polling, to ensure that we poll at least once before flushing, even if flush_rounds is 1.
        if i % flush_rounds == 0 {
            // flush and create a new buffer
            let prev_length = buffer.len(); // hint for the new buffer size, great if the number of measurements per flush doesn't change much
            tx.try_send(buffer).expect("todo: handle failed send (source too fast)");
            buffer = MeasurementBuffer::with_capacity(prev_length);
            log::debug!("source flushed {prev_length} measurements");

            // update state based on the latest command
            if commands.has_changed().unwrap() {
                let mut paused = false;
                'pause: loop {
                    let cmd = if paused {
                        commands
                            .changed()
                            .await
                            .expect("The output channel of paused source should be open.");
                        (*commands.borrow()).clone()
                    } else {
                        (*commands.borrow_and_update()).clone()
                    };
                    match cmd {
                        SourceCmd::Run => break 'pause,
                        SourceCmd::Pause => paused = true,
                        SourceCmd::Stop => break 'run,
                        SourceCmd::SetTrigger(mut opt) => {
                            (trigger, flush_rounds) = init_trigger(&mut opt)?;
                            let hint_additional_elems = flush_rounds - (i % flush_rounds);
                            buffer.reserve(hint_additional_elems);
                            if !paused {
                                break 'pause;
                            }
                        }
                    }
                }
            }
        }
        i = i.wrapping_add(1);
    }
    Ok(())
}

#[derive(Debug)]
pub enum TransformCmd {
    Enable,
    Disable,
}

async fn run_transforms(
    mut transforms: Vec<Box<dyn Transform>>,
    mut rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<MeasurementBuffer>,
    active_flags: Arc<AtomicU64>,
) -> Result<(), TransformError> {
    loop {
        if let Some(mut measurements) = rx.recv().await {
            // Update the list of active transforms (the PipelineController can update the flags).
            let current_flags = active_flags.load(Ordering::Relaxed);
            
            // Run the enabled transforms. If one of them fails, we cannot continue.
            for (i, t) in &mut transforms.iter_mut().enumerate() {
                let t_flag = 1 << i;
                if current_flags & t_flag != 0 {
                    t.apply(&mut measurements)?;
                }
            }
            
            // Send the results to the outputs.
            tx.send(measurements).map_err(|e| {
                TransformError::with_source(TransformErrorKind::Unrecoverable, "sending the measurements failed", e)
            })?;
        } else {
            log::warn!("The channel connected to the transform step has been closed, the transforms will stop.");
            break;
        }
    }
    Ok(())
}

/// A command for an output.
#[derive(Clone, PartialEq, Eq)]
pub enum OutputCmd {
    Run,
    Pause,
    Stop,
}

async fn run_output_from_broadcast(
    mut output: Box<dyn Output>,
    mut rx: broadcast::Receiver<MeasurementBuffer>,
    mut commands: watch::Receiver<OutputCmd>,
) -> Result<(), WriteError> {
    // Two possible designs:
    // A) Use one mpsc channel + one shared variable that contains the current command,
    // - when a message is received, check the command and act accordingly
    // - to change the command, update the variable and send a special message through the channel
    // In this alternative design, each Output would have one mpsc channel, and the Transform step would call send() or try_send() on each of them.
    //
    // B) use a broadcast + watch, where the broadcast discards old values when a receiver (output) lags behind,
    // instead of either (with option A):
    // - preventing the transform from running (mpsc channel's send() blocks when the queue is full).
    // - losing the most recent messages in transform, for one output. Other outputs that are not lagging behind will receive all messages fine, since try_send() does not block, the problem is: what to do with messages that could not be sent, when try_send() fails?)
    loop {
        tokio::select! {
            received_cmd = commands.changed() => {
                // Process new command, clone it to quickly end the borrow (which releases the internal lock as suggested by the doc)
                match received_cmd.map(|_| commands.borrow().clone()) {
                    Ok(OutputCmd::Run) => (), // continue running
                    Ok(OutputCmd::Pause) => {
                        // wait for the command to change
                        match commands.wait_for(|cmd| cmd != &OutputCmd::Pause).await {
                            Ok(new_cmd) => match *new_cmd {
                                OutputCmd::Run => (), // exit the wait
                                OutputCmd::Stop => break, // stop the loop
                                OutputCmd::Pause => unreachable!(),
                            },
                            Err(_) => todo!("watch channel closed"),
                        }
                    },
                    Ok(OutputCmd::Stop) => break, // stop the loop
                    Err(_) => todo!("watch channel closed")
                }
            },
            received_msg = rx.recv() => {
                match received_msg {
                    Ok(measurements) => {
                        // output.write() is blocking, do it in a dedicated thread
                        // Output is not Sync, move the value to the future and back
                        let res = tokio::task::spawn_blocking(move || {
                            (output.write(&measurements), output)
                        }).await;
                        match res {
                            Ok((write_res, out)) => {
                                output = out;
                                if let Err(e) = write_res {
                                    log::error!("Output failed: {:?}", e); // todo give a name to the output
                                }
                            },
                            Err(await_err) => {
                                if await_err.is_panic() {
                                    return Err(WriteError::with_source(super::WriteErrorKind::Unrecoverable, "The blocking writing task panicked.", await_err))
                                } else {
                                    todo!("unhandled error")
                                }
                            },
                        }
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("Output is too slow, it lost the oldest {n} messages.");
                    },
                    Err(broadcast::error::RecvError::Closed) => {
                        log::warn!("The channel connected to output was closed, it will now stop.");
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
