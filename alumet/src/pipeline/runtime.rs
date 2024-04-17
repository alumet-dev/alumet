//! Implementation of the measurement pipeline.

use std::collections::HashMap;
use std::io;
use std::ops::BitOrAssign;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{anyhow, Context};

use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio::{runtime::Runtime, sync::watch};
use tokio_stream::StreamExt;

use crate::metrics::{Metric, RawMetricId};
use crate::pipeline::scoped;
use crate::units::CustomUnitRegistry;
use crate::{
    measurement::MeasurementBuffer,
    metrics::MetricRegistry,
    pipeline::{Output, Source, Transform},
};

use super::registry::ElementRegistry;
use super::trigger::{ConfiguredTrigger, SourceTrigger, TriggerProvider};
use super::{threading, OutputContext, PollError, TransformError, WriteError};

/// A measurement pipeline that has not been started yet.
/// Use [`start`](Self::start) to launch it.
pub struct MeasurementPipeline {
    elements: PipelineElements,
    params: PipelineParameters,
}
/// The elements of a measurement pipeline, with all required information (e.g. source triggers).
struct PipelineElements {
    sources: Vec<ConfiguredSource>,
    transforms: Vec<ConfiguredTransform>,
    outputs: Vec<ConfiguredOutput>,

    // Channel: outputs -> listeners of late metric registration
    late_reg_res_tx: mpsc::Sender<Vec<RawMetricId>>,
    late_reg_res_rx: mpsc::Receiver<Vec<RawMetricId>>,
}
/// Parameters of the measurement pipeline.
struct PipelineParameters {
    normal_worker_threads: Option<usize>,
    priority_worker_threads: Option<usize>,
}

/// The type of a [`Source`].
/// 
/// It affects how Alumet schedules the polling of the source.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceType {
    /// Nothing special. This is the right choice for most of the sources.
    Normal,
    // Blocking, // todo: how to provide this type properly?

    /// Signals that the pipeline should run the source on a thread with a
    /// high scheduling priority.
    RealtimePriority,
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
    pub trigger_provider: TriggerProvider,
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

/// A message for an element of the pipeline.
enum ControlMessage {
    Source(Option<String>, SourceCmd),
    Output(Option<String>, OutputCmd),
    Transform(Option<String>, TransformCmd),
}

/// A `PipelineController` allows to dynamically change the configuration of a running measurement pipeline.
///
/// Dropping the controller aborts all the tasks of the pipeline (the internal Tokio [`Runtime`]s are dropped).
/// To keep the pipeline running, use [`wait_for_all`](RunningPipeline::wait_for_all).
pub struct RunningPipeline {
    // Keep the tokio runtimes alive
    normal_runtime: Runtime,
    _priority_runtime: Option<Runtime>,

    // Handles to wait for pipeline elements to finish.
    source_handles: Vec<JoinHandle<Result<(), PollError>>>,
    output_handles: Vec<JoinHandle<Result<(), WriteError>>>,
    transform_handle: JoinHandle<Result<(), TransformError>>,

    // Controller, initially Some, taken (replaced by None) by the control task started by the first control handle.
    controller: Option<PipelineController>,

    // Control handle, initially None, set by the first call to `control_handle`.
    control_handle: Option<ControlHandle>,
}
struct PipelineController {
    // Senders to keep the receivers alive and to send commands.
    source_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<SourceCmd>>>,
    output_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<OutputCmd>>>,

    /// Currently active transforms.
    /// Note: it could be generalized to support more than 64 values,
    /// either with a crate like arc-swap, or by using multiple Vec of transforms, each with an AtomicU64.
    active_transforms: Arc<AtomicU64>,
    transforms_mask_by_plugin: HashMap<String, u64>,
}
#[derive(Clone)]
pub struct ControlHandle {
    tx: mpsc::Sender<ControlMessage>,
}

impl MeasurementPipeline {
    /// Creates a new measurement pipeline with the elements in the registry and some additional settings applied to the sources
    /// by the function `f`.
    ///
    /// The returned pipeline is not started, use [`start`](Self::start) to start it.
    pub fn with_settings<F>(elements: ElementRegistry, mut f: F) -> MeasurementPipeline
    where
        F: FnMut(Box<dyn Source>, String) -> ConfiguredSource,
    {
        let sources: Vec<_> = elements.sources.into_iter().map(|(s, p)| f(s, p)).collect();
        MeasurementPipeline {
            elements: PipelineElements {
                sources,
                transforms: elements.transforms,
                outputs: elements.outputs,
                late_reg_res_tx: elements.late_reg_res_tx,
                late_reg_res_rx: elements.late_reg_res_rx,
            },
            params: PipelineParameters {
                normal_worker_threads: None,
                priority_worker_threads: None,
            },
        }
    }

    /// Sets the number of threads in the "normal" tokio [`Runtime`].
    /// No more than `n` sources of type [`SourceType::Normal`] can be polled at the same time.
    pub fn normal_worker_threads(&mut self, n: usize) {
        self.params.normal_worker_threads = Some(n);
    }
    /// Sets the number of threads in the "priority" tokio [`Runtime`], which runs the sources of type [`SourceType::RealtimePriority`].
    pub fn priority_worker_threads(&mut self, n: usize) {
        self.params.priority_worker_threads = Some(n);
    }

    /// Starts the measurement pipeline and returns a controller for it.
    pub fn start(self, metrics: MetricRegistry, units: CustomUnitRegistry) -> RunningPipeline {
        // Set the global registries, which can be accessed by the pipeline's elements (sources, transforms, outputs).
        CustomUnitRegistry::init_global(units);

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

        // Channel: sources -> transforms
        let (in_tx, in_rx) = mpsc::channel::<MeasurementBuffer>(256);

        // if self.elements.transforms.is_empty() && self.elements.outputs.len() == 1 {
        // TODO: If no transforms and one output, the pipeline can be reduced
        // }

        // Broadcast queue: transforms -> outputs
        let out_tx = broadcast::Sender::<OutputMsg>::new(256);

        // Store the task handles in order to wait for them to complete before stopping,
        // and the command senders in order to keep the receivers alive and to be able to send commands after the launch.
        let mut source_handles = Vec::with_capacity(self.elements.sources.len());
        let mut output_handles = Vec::with_capacity(self.elements.outputs.len());
        let mut source_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut output_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut transforms_mask_by_plugin: HashMap<_, u64> = HashMap::new();

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).
        // 1. Outputs
        for out in self.elements.outputs {
            let msg_rx = out_tx.subscribe();
            let (command_tx, command_rx) = watch::channel(OutputCmd::Run);
            let ctx = OutputContext {
                // Each output task owns its OutputContext, which contains a copy of the MetricRegistry.
                // This allows fast, uncontended access to the registry, and avoids a global state (no Arc<Mutex<...>>).
                // The cost is a duplication of the registry (increased memory use) in the case where multiple outputs exist.
                metrics: metrics.clone(),
            };
            // Spawn the task and store the handle.
            let handle = normal_runtime.spawn(run_output_from_broadcast(out.output, msg_rx, command_rx, ctx));
            output_handles.push(handle);
            output_command_senders_by_plugin
                .entry(out.plugin_name)
                .or_default()
                .push(command_tx);
        }

        // 2. Transforms
        let active_transforms = Arc::new(AtomicU64::new(u64::MAX)); // all active by default
        let mut transforms = Vec::with_capacity(self.elements.transforms.len());
        for (i, t) in self.elements.transforms.into_iter().enumerate() {
            transforms.push(t.transform);
            let mask: u64 = 1 << i;
            transforms_mask_by_plugin
                .entry(t.plugin_name)
                .or_default()
                .bitor_assign(mask);
        }
        let transform_handle =
            normal_runtime.spawn(run_transforms(transforms, in_rx, out_tx, active_transforms.clone()));

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
            source_command_senders_by_plugin
                .entry(src.plugin_name)
                .or_default()
                .push(command_tx);
        }

        RunningPipeline {
            normal_runtime,
            _priority_runtime: priority_runtime,
            source_handles,
            output_handles,
            transform_handle,

            controller: Some(PipelineController {
                source_command_senders_by_plugin,
                output_command_senders_by_plugin,
                active_transforms,
                transforms_mask_by_plugin,
            }),
            control_handle: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum SourceCmd {
    Run,
    Pause,
    Stop,
    SetTrigger(Option<TriggerProvider>),
}

async fn run_source(
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    mut commands: watch::Receiver<SourceCmd>,
) -> Result<(), PollError> {
    fn init_trigger(provider: &mut Option<TriggerProvider>) -> anyhow::Result<ConfiguredTrigger> {
        provider
            .take()
            .expect("invalid empty trigger in message Init(trigger)")
            .auto_configured()
            .context("init_trigger failed")
    }

    // the first command must be "init"
    let mut trigger = {
        let init_cmd = commands
            .wait_for(|c| matches!(c, SourceCmd::SetTrigger(_)))
            .await
            .expect("watch channel must stay open during run_source");

        match (*init_cmd).clone() {
            // cloning required to borrow opt as mut below
            SourceCmd::SetTrigger(mut opt) => init_trigger(&mut opt)?,
            _ => unreachable!(),
        }
    };

    // Stores measurements in this buffer, and replace it every `flush_rounds` rounds.
    // We probably need the capacity to store at least one measurement per round.
    let mut buffer = MeasurementBuffer::with_capacity(trigger.flush_rounds);

    // main loop
    let mut i = 1usize;
    'run: loop {
        // wait for trigger
        match trigger.trigger {
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
        if i % trigger.flush_rounds == 0 {
            // flush and create a new buffer
            let prev_length = buffer.len(); // hint for the new buffer size, great if the number of measurements per flush doesn't change much
            tx.try_send(buffer)
                .context("todo: handle failed send (source too fast)")?;
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
                            trigger = init_trigger(&mut opt)?;
                            let hint_additional_elems = trigger.flush_rounds - (i % trigger.flush_rounds);
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
    tx: broadcast::Sender<OutputMsg>,
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
            tx.send(OutputMsg::WriteMeasurements(measurements))
                .context("could not send the measurements from transforms to the outputs")?;
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

#[derive(Debug, Clone)]
pub enum OutputMsg {
    WriteMeasurements(MeasurementBuffer),
    RegisterMetrics {
        metrics: Vec<Metric>,
        source_name: String,
        reply_to: tokio::sync::mpsc::Sender<Vec<RawMetricId>>,
    },
}

async fn run_output_from_broadcast(
    mut output: Box<dyn Output>,
    mut rx: broadcast::Receiver<OutputMsg>,
    mut commands: watch::Receiver<OutputCmd>,
    mut ctx: OutputContext,
) -> Result<(), WriteError> {
    // Two possible designs:
    // A) Use one mpsc channel + one shared variable that contains the current command,
    // - when a message is received, check the command and act accordingly
    // - to change the command, update the variable and send a special message through the channel
    // In this alternative design, each Output would have one mpsc channel, and the Transform step would call send() or try_send() on each of them.
    //
    // B) use a broadcast + watch, where the broadcast discards old values when a receiver (output) lags behind, instead of either (with option A):
    // - preventing the transform from running (mpsc channel's send() blocks when the queue is full).
    // - losing the most recent messages in transform, for one output. Other outputs that are not lagging behind will receive all messages fine, since try_send() does not block.
    //     The problem is: what to do with messages that could not be sent, when try_send() fails?

    async fn handle_message(received_msg: OutputMsg, output: &mut dyn Output, ctx: &mut OutputContext) -> Result<(), WriteError> {
        match received_msg {
            OutputMsg::WriteMeasurements(measurements) => {
                // output.write() is blocking, do it in a dedicated thread.

                // Output is not Sync, we could move the value to the future and back (idem for ctx),
                // but that would likely introduce a needless copy, and would be cumbersome to work with.
                // Instead, we use the `scoped` module.
                let res = scoped::spawn_blocking_with_output(output, ctx, move |out, ctx| out.write(&measurements, &ctx)).await;
                match res {
                    Ok(write_res) => {
                        if let Err(e) = write_res {
                            log::error!("Output failed: {:?}", e); // todo give a name to the output
                        }
                        Ok(())
                    },
                    Err(await_err) => {
                        if await_err.is_panic() {
                            return Err(anyhow!("A blocking writing task panicked, there is a bug somewhere! Details: {}", await_err).into());
                        } else {
                            todo!("unhandled error");
                        }
                    },
                }
            },
            OutputMsg::RegisterMetrics { metrics, source_name, reply_to } => {
                let metric_ids = ctx.metrics.extend_infallible(metrics, &source_name);
                reply_to.send(metric_ids).await?;
                Ok(())
            },
        }
    }

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
                        };
                    },
                    Ok(OutputCmd::Stop) => break, // stop the loop
                    Err(_) => todo!("watch channel closed")
                }
            },
            received_msg = rx.recv() => {
                match received_msg {
                    Ok(msg) => {
                        handle_message(msg, output.as_mut(), &mut ctx).await?;
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

impl RunningPipeline {
    /// Blocks the current thread until all tasks in the pipeline finish.
    pub fn wait_for_all(&mut self) {
        self.normal_runtime.block_on(async {
            for handle in &mut self.source_handles {
                handle.await.unwrap().unwrap(); // todo: handle errors
            }

            (&mut self.transform_handle).await.unwrap().unwrap();

            for handle in &mut self.output_handles {
                handle.await.unwrap().unwrap();
            }
        });
    }

    /// Returns a [`ControlHandle`], which allows to change the configuration
    /// of the pipeline while it is running.
    pub fn control_handle(&mut self) -> ControlHandle {
        fn handle_message(controller: &mut PipelineController, msg: ControlMessage) {
            match msg {
                ControlMessage::Source(plugin_name, cmd) => {
                    if let Some(plugin) = plugin_name {
                        for s in controller.source_command_senders_by_plugin.get(&plugin).unwrap() {
                            s.send(cmd.clone()).unwrap();
                        }
                    } else {
                        for senders in controller.source_command_senders_by_plugin.values() {
                            for s in senders {
                                s.send(cmd.clone()).unwrap();
                            }
                        }
                    }
                }
                ControlMessage::Output(plugin_name, cmd) => {
                    if let Some(plugin) = plugin_name {
                        for s in controller.output_command_senders_by_plugin.get(&plugin).unwrap() {
                            s.send(cmd.clone()).unwrap();
                        }
                    } else {
                        for senders in controller.output_command_senders_by_plugin.values() {
                            for s in senders {
                                s.send(cmd.clone()).unwrap();
                            }
                        }
                    }
                }
                ControlMessage::Transform(plugin_name, cmd) => {
                    let mask: u64 = if let Some(plugin) = plugin_name {
                        *controller.transforms_mask_by_plugin.get(&plugin).unwrap()
                    } else {
                        u64::MAX
                    };
                    match cmd {
                        TransformCmd::Enable => controller.active_transforms.fetch_or(mask, Ordering::Relaxed),
                        TransformCmd::Disable => controller.active_transforms.fetch_nand(mask, Ordering::Relaxed),
                    };
                }
            }
        }

        match self.controller.take() {
            Some(mut controller) => {
                // This is the first handle, starts the control task.
                let (tx, mut rx) = mpsc::channel::<ControlMessage>(256);
                self.normal_runtime.spawn(async move {
                    loop {
                        if let Some(msg) = rx.recv().await {
                            handle_message(&mut controller, msg);
                        } else {
                            break; // channel closed
                        }
                    }
                });
                let handle = ControlHandle { tx };
                self.control_handle = Some(handle.clone());
                handle
            }
            None => {
                // This is NOT the first handle, get the existing handle and clone it.
                self.control_handle.as_ref().unwrap().clone()
            }
        }
    }
}

impl ControlHandle {
    pub fn all(&self) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            plugin_name: None,
        }
    }
    pub fn plugin(&self, plugin_name: impl Into<String>) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            plugin_name: Some(plugin_name.into()),
        }
    }

    pub fn blocking_all(&self) -> BlockingControlHandle {
        BlockingControlHandle {
            handle: self,
            plugin_name: None,
        }
    }

    pub fn blocking_plugin(&self, plugin_name: impl Into<String>) -> BlockingControlHandle {
        BlockingControlHandle {
            handle: self,
            plugin_name: Some(plugin_name.into()),
        }
    }
}

pub struct ScopedControlHandle<'a> {
    handle: &'a ControlHandle,
    plugin_name: Option<String>,
}
impl<'a> ScopedControlHandle<'a> {
    pub async fn control_sources(self, cmd: SourceCmd) {
        self.handle
            .tx
            .send(ControlMessage::Source(self.plugin_name, cmd))
            .await
            .unwrap();
    }
    pub async fn control_transforms(self, cmd: TransformCmd) {
        self.handle
            .tx
            .send(ControlMessage::Transform(self.plugin_name, cmd))
            .await
            .unwrap();
    }
    pub async fn control_outputs(self, cmd: OutputCmd) {
        self.handle
            .tx
            .send(ControlMessage::Output(self.plugin_name, cmd))
            .await
            .unwrap();
    }
}

pub struct BlockingControlHandle<'a> {
    handle: &'a ControlHandle,
    plugin_name: Option<String>,
}
impl<'a> BlockingControlHandle<'a> {
    pub fn control_sources(self, cmd: SourceCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::Source(self.plugin_name, cmd))
            .unwrap();
    }
    pub fn control_transforms(self, cmd: TransformCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::Transform(self.plugin_name, cmd))
            .unwrap();
    }
    pub fn control_outputs(self, cmd: OutputCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::Output(self.plugin_name, cmd))
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
            Arc,
        },
        thread::sleep,
        time::{Duration, Instant},
    };

    use tokio::{
        runtime::Runtime,
        sync::{broadcast, mpsc, watch},
    };

    use crate::{
        measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementType, WrappedMeasurementValue},
        metrics::{MetricRegistry, RawMetricId},
        pipeline::{trigger::TriggerProvider, OutputContext, Transform},
        resources::ResourceId,
    };

    use super::{run_output_from_broadcast, run_source, run_transforms, OutputCmd, OutputMsg, SourceCmd};

    #[test]
    fn source_triggered_by_time() {
        let rt = new_rt(2);

        let source = TestSource::new();
        let tp = new_tp();
        let (tx, mut rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (cmd_tx, cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        let stopped = Arc::new(AtomicU8::new(TestSourceState::Running as _));
        let stopped2 = stopped.clone();
        rt.spawn(async move {
            let mut n_polls = 0;
            loop {
                // check the measurements
                if let Some(measurements) = rx.recv().await {
                    assert_ne!(
                        TestSourceState::Stopped as u8,
                        stopped2.load(Ordering::Relaxed),
                        "The source is stopped/paused, it should not produce measurements."
                    );

                    // 2 by 2 because flush_interval = 2*poll_interval
                    assert_eq!(measurements.len(), 2);
                    n_polls += 2;
                    let last_point = measurements.iter().last().unwrap();
                    let last_point_value = match last_point.value {
                        WrappedMeasurementValue::U64(n) => n,
                        _ => panic!("unexpected value type"),
                    };
                    assert_eq!(n_polls, last_point_value);
                } else {
                    // the channel is dropped when run_source terminates, which must only occur when the source is stopped
                    assert_ne!(
                        TestSourceState::Running as u8,
                        stopped2.load(Ordering::Relaxed),
                        "The source is not stopped, the channel should be open."
                    );
                }
            }
        });

        // poll the source for some time
        rt.spawn(run_source(Box::new(source), tx, cmd_rx));
        sleep(Duration::from_millis(20));

        // pause source
        cmd_tx.send(SourceCmd::Pause).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(Duration::from_millis(10)); // some tolerance (wait for flushing)
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is paused
        sleep(Duration::from_millis(10));

        // still paused after SetTrigger
        cmd_tx.send(SourceCmd::SetTrigger(Some(new_tp()))).unwrap();
        sleep(Duration::from_millis(20));

        // resume source
        cmd_tx.send(SourceCmd::Run).unwrap();
        stopped.store(TestSourceState::Running as _, Ordering::Relaxed);
        sleep(Duration::from_millis(5)); // lower tolerance (no flushing, just waiting for changes on the watch channel)

        // poll for some time
        sleep(Duration::from_millis(10));

        // still running after SetTrigger
        cmd_tx.send(SourceCmd::SetTrigger(Some(new_tp()))).unwrap();
        sleep(Duration::from_millis(20));

        // stop source
        cmd_tx.send(SourceCmd::Stop).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(Duration::from_millis(10)); // some tolerance
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is stopped
        sleep(Duration::from_millis(20));

        // drop the runtime, abort the tasks
    }

    #[test]
    fn transform_task() {
        let rt = new_rt(2);

        // create transforms
        let check_input_type_for_transform3 = Arc::new(AtomicBool::new(true));
        let transforms: Vec<Box<dyn Transform>> = vec![
            Box::new(TestTransform {
                id: 1,
                expected_input_len: 2, // 2 because flush_interval = 2*poll_interval
                output_type: WrappedMeasurementType::U64,
                expected_input_type: WrappedMeasurementType::U64,
                check_input_type: Arc::new(AtomicBool::new(true)),
            }),
            Box::new(TestTransform {
                id: 2,
                expected_input_len: 2,
                output_type: WrappedMeasurementType::F64,
                expected_input_type: WrappedMeasurementType::U64,
                check_input_type: Arc::new(AtomicBool::new(true)),
            }),
            Box::new(TestTransform {
                id: 3,
                expected_input_len: 2,
                output_type: WrappedMeasurementType::F64,
                expected_input_type: WrappedMeasurementType::F64,
                check_input_type: check_input_type_for_transform3.clone(),
            }),
        ];

        // create source
        let source = TestSource::new();
        let tp = new_tp();
        let (src_tx, src_rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (_src_cmd_tx, src_cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        // create transform channels and control flags
        let (trans_tx, mut out_rx) = broadcast::channel::<OutputMsg>(64);
        let active_flags = Arc::new(AtomicU64::new(u64::MAX));
        let active_flags2 = active_flags.clone();
        let active_flags3 = active_flags.clone();

        rt.spawn(async move {
            loop {
                if let Ok(OutputMsg::WriteMeasurements(measurements)) = out_rx.recv().await {
                    let current_flags = active_flags2.load(Ordering::Relaxed);
                    let transform1_enabled = current_flags & 1 != 0;
                    let transform2_enabled = current_flags & 2 != 0;
                    let transform3_enabled = current_flags & 4 != 0;
                    for m in measurements.iter() {
                        let int_val = match m.value {
                            WrappedMeasurementValue::F64(f) => f as u32,
                            WrappedMeasurementValue::U64(u) => u as u32,
                        };
                        if transform3_enabled {
                            assert_eq!(int_val, 3);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::F64);
                        } else if transform2_enabled {
                            assert_eq!(int_val, 2);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::F64);
                        } else if transform1_enabled {
                            assert_eq!(int_val, 1);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::U64);
                        } else {
                            assert_ne!(int_val, 3);
                            assert_ne!(int_val, 2);
                            assert_ne!(int_val, 1);
                            assert_eq!(m.value.measurement_type(), WrappedMeasurementType::U64);
                        }
                    }
                }
            }
        });

        // run the transforms
        rt.spawn(run_transforms(transforms, src_rx, trans_tx, active_flags3));

        // poll the source for some time
        rt.spawn(run_source(Box::new(source), src_tx, src_cmd_rx));
        sleep(Duration::from_millis(20));

        // disable transform 3 only
        active_flags.store(1 | 2, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable transform 1 only
        active_flags.store(2 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable transform 2 only, the input type expected by transform 3 is no longer respected
        check_input_type_for_transform3.store(false, Ordering::Relaxed);
        active_flags.store(1 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // disable all transforms
        active_flags.store(0, Ordering::Relaxed);
        check_input_type_for_transform3.store(true, Ordering::Relaxed);
        sleep(Duration::from_millis(20));

        // enable all transforms
        active_flags.store(1 | 2 | 4, Ordering::Relaxed);
        sleep(Duration::from_millis(20));
    }

    #[test]
    fn output_task() {
        let rt = new_rt(3);
        // create source
        let source = Box::new(TestSource::new());
        let tp = new_tp();
        let (src_tx, trans_rx) = mpsc::channel::<MeasurementBuffer>(64);
        let (src_cmd_tx, src_cmd_rx) = watch::channel(SourceCmd::SetTrigger(Some(tp)));

        // no transforms but a transform task to send the values to the output
        let transforms = vec![];
        let (trans_tx, out_rx) = broadcast::channel::<OutputMsg>(64);
        let active_flags = Arc::new(AtomicU64::new(u64::MAX));

        // create output
        let output_count = Arc::new(AtomicU32::new(0));
        let output = Box::new(TestOutput {
            expected_input_len: 2,
            output_count: output_count.clone(),
        });
        let (out_cmd_tx, out_cmd_rx) = watch::channel(OutputCmd::Run);
        let out_ctx = OutputContext {
            metrics: MetricRegistry::new(),
        };

        // start tasks
        rt.spawn(run_output_from_broadcast(output, out_rx, out_cmd_rx, out_ctx));
        rt.spawn(run_transforms(transforms, trans_rx, trans_tx, active_flags));
        rt.spawn(run_source(source, src_tx, src_cmd_rx));

        // check the output
        sleep(Duration::from_millis(20));
        assert!(output_count.load(Ordering::Relaxed).abs_diff(4) <= 2);

        // pause and check
        out_cmd_tx.send(OutputCmd::Pause).unwrap();
        let count_at_pause = output_count.load(Ordering::Relaxed);
        sleep(Duration::from_millis(10));
        assert!(output_count.load(Ordering::Relaxed).abs_diff(count_at_pause) <= 2);
        sleep(Duration::from_millis(20));

        // resume and check
        let count_before_resume = output_count.load(Ordering::Relaxed);
        out_cmd_tx.send(OutputCmd::Run).unwrap();
        sleep(Duration::from_millis(20));
        assert!(output_count.load(Ordering::Relaxed) > count_before_resume);

        // stop and check
        src_cmd_tx.send(SourceCmd::Stop).unwrap();
        out_cmd_tx.send(OutputCmd::Stop).unwrap();
        sleep(Duration::from_millis(10));
        let count = output_count.load(Ordering::Relaxed);
        sleep(Duration::from_millis(20));
        assert_eq!(count, output_count.load(Ordering::Relaxed));
    }

    fn new_tp() -> TriggerProvider {
        TriggerProvider::TimeInterval {
            start_time: Instant::now(),
            poll_interval: Duration::from_millis(5),
            flush_interval: Duration::from_millis(10),
        }
    }

    fn new_rt(n_threads: usize) -> Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(n_threads)
            .enable_all()
            .build()
            .unwrap()
    }

    enum TestSourceState {
        /// The source must be running.
        Running = 0,
        /// The source has been stopped but can continue to run for some time
        /// before the next refresh of the commands (which occurs at the same time as flushing).
        Stopping = 1,
        /// The source has been stopped and should have been refreshed, no measurements must be produced.
        Stopped = 2,
    }

    struct TestSource {
        n_calls: u32,
    }
    impl TestSource {
        fn new() -> TestSource {
            TestSource { n_calls: 0 }
        }
    }
    impl crate::pipeline::Source for TestSource {
        fn poll(
            &mut self,
            into: &mut crate::measurement::MeasurementAccumulator,
            time: std::time::SystemTime,
        ) -> Result<(), crate::pipeline::PollError> {
            self.n_calls += 1;
            let point = MeasurementPoint::new_untyped(
                time,
                RawMetricId(1),
                ResourceId::LocalMachine,
                WrappedMeasurementValue::U64(self.n_calls as u64),
            );
            into.push(point);
            Ok(())
        }
    }

    struct TestTransform {
        id: u32,
        output_type: WrappedMeasurementType,
        expected_input_len: usize,
        expected_input_type: WrappedMeasurementType,
        check_input_type: Arc<AtomicBool>,
    }

    impl crate::pipeline::Transform for TestTransform {
        fn apply(&mut self, measurements: &mut MeasurementBuffer) -> Result<(), crate::pipeline::TransformError> {
            assert_eq!(measurements.len(), self.expected_input_len);
            for m in measurements.iter_mut() {
                assert_eq!(m.resource, ResourceId::LocalMachine);
                if self.check_input_type.load(Ordering::Relaxed) {
                    assert_eq!(m.value.measurement_type(), self.expected_input_type);
                }
                m.value = match self.output_type {
                    WrappedMeasurementType::F64 => WrappedMeasurementValue::F64(self.id as _),
                    WrappedMeasurementType::U64 => WrappedMeasurementValue::U64(self.id as _),
                };
            }
            assert_eq!(measurements.len(), self.expected_input_len);
            Ok(())
        }
    }

    struct TestOutput {
        expected_input_len: usize,
        output_count: Arc<AtomicU32>,
    }

    impl crate::pipeline::Output for TestOutput {
        fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), crate::pipeline::WriteError> {
            assert_eq!(measurements.len(), self.expected_input_len);
            self.output_count.fetch_add(measurements.len() as _, Ordering::Relaxed);
            Ok(())
        }
    }
}
