//! Implementation of the measurement pipeline.

use std::collections::HashMap;
use std::ops::BitOrAssign;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};

use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, mpsc};
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::timeout;
use tokio::{runtime::Runtime, sync::watch};
use tokio_util::sync::CancellationToken;

use crate::measurement::Timestamp;
use crate::metrics::{Metric, RawMetricId};
use crate::pipeline::scoped;
use crate::pipeline::trigger::TriggerReason;
use crate::{
    measurement::MeasurementBuffer,
    metrics::MetricRegistry,
    pipeline::{Output, Source},
};

use super::builder;
use super::builder::{ConfiguredTransform, ElementType};
use super::trigger::{Trigger, TriggerSpec};
use super::{OutputContext, PollError, TransformError, WriteError};

/// A measurement pipeline that has not been started yet.
pub struct IdlePipeline {
    // Elements of the pipeline
    pub(super) sources: Vec<builder::ConfiguredSource>,
    pub(super) transforms: Vec<builder::ConfiguredTransform>,
    pub(super) outputs: Vec<builder::ConfiguredOutput>,
    pub(super) autonomous_sources: Vec<builder::ConfiguredAutonomousSource>,

    // Cancellation token to implement the graceful shutdown of autonomous sources.
    pub(super) autonomous_shutdown_token: CancellationToken,

    // tokio Runtimes that execute the tasks
    pub(super) rt_normal: Runtime,
    pub(super) rt_priority: Option<Runtime>,

    // registries
    pub(super) metrics: MetricRegistry,

    /// Channel: source -> transforms
    pub(super) from_sources: (mpsc::Sender<MeasurementBuffer>, mpsc::Receiver<MeasurementBuffer>),

    /// Broadcast queue to outputs
    pub(super) to_outputs: broadcast::Sender<OutputMsg>,
}

/// A message to control the pipeline.
enum ControlMessage {
    Shutdown,
    AddSource {
        requested_name: String,
        plugin_name: String,
        source: Box<dyn Source>,
        trigger: TriggerSpec,
    },
    ModifySource(ElementCommand<SourceCmd>),
    ModifyTransform(ElementCommand<TransformCmd>),
    ModifyOutput(ElementCommand<OutputCmd>),
}

/// A command sent to one or multiple elements of the pipeline.
struct ElementCommand<T> {
    destination: MessageDestination,
    command: T,
}

/// Specifies the destination of the [`ElementCommand`].
#[derive(Clone)]
enum MessageDestination {
    /// Send the command to all the elements of this type
    /// (e.g. all sources in case of an [`ElementCommand<SourceCmd>`]).
    All,
    /// Send the command to all the elements of this type registered by a specific plugin.
    Plugin(String),
}

/// A measurement pipeline that is currently running.
pub struct RunningPipeline {
    // Keep the tokio runtimes alive
    _rt_normal: Runtime,
    _rt_priority: Option<Runtime>,

    /// Handle to the task that handles the shutdown of the pipeline.
    ///
    /// When this task finishes, the pipeline has shut down.
    shutdown_task_handle: Option<JoinHandle<()>>,

    /// Controls the pipeline.
    control_handle: ControlHandle,
}

struct PipelineControllerState {
    /// Send a message to this channel in order to shutdown the entire pipeline.
    global_shutdown_send: UnboundedSender<()>,

    // Senders to keep the receivers alive and to send commands.
    source_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<SourceCmd>>>,
    output_command_senders_by_plugin: HashMap<String, Vec<watch::Sender<OutputCmd>>>,

    /// Currently active transforms.
    /// Note: it could be generalized to support more than 64 values,
    /// either with a crate like arc-swap, or by using multiple Vec of transforms, each with an AtomicU64.
    active_transforms: Arc<AtomicU64>,
    transforms_mask_by_plugin: HashMap<String, u64>,

    // Allows to shut the autonomous sources down.
    autonomous_shutdown_token: CancellationToken,

    /// State useful for modifying the pipeline.
    modifier: PipelineModifierState,
}

/// Things necessary for modifying the pipeline at runtime,
/// that is, adding or removing pipeline elements.
struct PipelineModifierState {
    /// Name generator for the new sources.
    namegen: builder::ElementNameGenerator,

    /// All the JoinSets of the running pipeline.
    join_sets: ElementJoinSets,

    /// Sends measurements from Sources.
    in_tx: mpsc::Sender<MeasurementBuffer>,

    /// Handle to the tokio runtime with "normal" threads.
    rt_normal: tokio::runtime::Handle,
}

#[derive(Clone)]
pub struct ControlHandle {
    /// Send a message to this channel to control the pipeline.
    ///
    /// Closed when the pipeline shuts down.
    tx: mpsc::Sender<ControlMessage>,
}

impl IdlePipeline {
    pub fn metric_count(&self) -> usize {
        self.metrics.len()
    }

    pub fn metric_iter(&self) -> crate::metrics::MetricIter<'_> {
        self.metrics.iter()
    }

    /// Starts the measurement pipeline.
    pub fn start(self) -> RunningPipeline {
        // Use a JoinSet to keep track of the spawned tasks.
        let mut source_set = JoinSet::new();
        let mut transform_set = JoinSet::new();
        let mut output_set = JoinSet::new();

        // Store the command senders in order to keep the receivers alive,
        // and to be able to send commands after the launch.
        let mut source_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut output_command_senders_by_plugin: HashMap<_, Vec<_>> = HashMap::new();
        let mut transforms_mask_by_plugin: HashMap<_, u64> = HashMap::new();

        // Start the tasks, starting at the end of the pipeline (to avoid filling the buffers too quickly).
        let (in_tx, in_rx) = self.from_sources;

        // 1. Outputs
        for out in self.outputs {
            let msg_rx = self.to_outputs.subscribe();
            let (command_tx, command_rx) = watch::channel(OutputCmd::Run);
            let ctx = OutputContext {
                // Each output task owns its OutputContext, which contains a copy of the MetricRegistry.
                // This allows fast, uncontended access to the registry, and avoids a global state (no Arc<Mutex<...>>).
                // The cost is a duplication of the registry (increased memory use) in the case where multiple outputs exist.
                metrics: self.metrics.clone(),
            };

            // Store command_tx so that we can accept commands later (commands can target the outputs of a specific plugin).
            output_command_senders_by_plugin
                .entry(out.plugin_name)
                .or_default()
                .push(command_tx);

            // Spawn the task in the JoinSet.
            let task = run_output_from_broadcast(out.name, out.output, msg_rx, command_rx, ctx);
            output_set.spawn_on(task, self.rt_normal.handle());
        }

        // 2. Transforms (all in the same task because they are applied one after another)
        let active_transforms = Arc::new(AtomicU64::new(u64::MAX)); // all active by default
        for (i, t) in self.transforms.iter().enumerate() {
            let mask: u64 = 1 << i;
            transforms_mask_by_plugin
                .entry(t.plugin_name.clone())
                .or_default()
                .bitor_assign(mask);
        }
        let transforms_task = run_transforms(self.transforms, in_rx, self.to_outputs, active_transforms.clone());
        transform_set.spawn_on(transforms_task, self.rt_normal.handle());

        // 3. Managed sources
        for src in self.sources {
            let data_tx = in_tx.clone();
            let runtime = match src.trigger_provider.realtime_priority {
                true => self.rt_priority.as_ref().unwrap_or(&self.rt_normal),
                false => &self.rt_normal,
            };
            let (command_tx, command_rx) = watch::channel(SourceCmd::SetTrigger(Some(src.trigger_provider)));
            source_command_senders_by_plugin
                .entry(src.plugin_name)
                .or_default()
                .push(command_tx);

            let task = run_source(src.name, src.source, data_tx, command_rx);
            source_set.spawn_on(task, runtime.handle());
        }

        // 4. Autonomous sources
        for src in self.autonomous_sources {
            let task = async move {
                src.source
                    .await
                    .map_err(|e| e.context(format!("error in autonomous source {}", src.name)))
            };
            source_set.spawn_on(task, self.rt_normal.handle());
        }

        // 5. Graceful shutdown and pipeline control.

        // mpsc channel for global shutdown order.
        let (global_shutdown_send, global_shutdown_recv) = mpsc::unbounded_channel::<()>();

        // Store the JoinSets to be able to wait for the tasks in a specific order (see pipeline_control_task).
        let join_sets = ElementJoinSets {
            source_set,
            transform_set,
            output_set,
        };

        // Spawn a task to control the pipeline and orchestrate its shutdown.
        // Most of the state (command senders, mask of the active transforms, etc.) is moved to this task.
        let (control_tx, control_rx) = mpsc::channel::<ControlMessage>(256);
        let controller_state = PipelineControllerState {
            global_shutdown_send,
            source_command_senders_by_plugin,
            output_command_senders_by_plugin,
            active_transforms,
            transforms_mask_by_plugin,
            autonomous_shutdown_token: self.autonomous_shutdown_token,
            modifier: PipelineModifierState {
                namegen: builder::ElementNameGenerator::new(),
                join_sets,
                in_tx,
                rt_normal: self.rt_normal.handle().clone(),
            },
        };
        let control_handle = ControlHandle { tx: control_tx };
        let control_task_handle = self.rt_normal.spawn(pipeline_control_task(
            global_shutdown_recv,
            control_rx,
            controller_state,
        ));

        RunningPipeline {
            _rt_normal: self.rt_normal,
            _rt_priority: self.rt_priority,
            shutdown_task_handle: Some(control_task_handle),
            control_handle,
        }
    }
}

/// Stores [`JoinSet`]s for all the tasks of the pipeline
/// that correspond to an element (source, transform, output).
struct ElementJoinSets {
    source_set: JoinSet<anyhow::Result<()>>,
    transform_set: JoinSet<anyhow::Result<()>>,
    output_set: JoinSet<anyhow::Result<()>>,
}

#[derive(Clone, Debug)]
pub enum SourceCmd {
    Run,
    Pause,
    Stop,
    SetTrigger(Option<TriggerSpec>),
}

async fn run_source(
    source_name: String,
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    mut commands: watch::Receiver<SourceCmd>,
) -> anyhow::Result<()> {
    /// Takes the [`Trigger`] from the option and initializes it.
    fn init_trigger(
        trigger_spec: &mut Option<TriggerSpec>,
        interrupt_signal: watch::Receiver<SourceCmd>,
    ) -> Result<Trigger, std::io::Error> {
        let spec = trigger_spec
            .take()
            .expect("invalid empty trigger in message Init(trigger)");
        Trigger::new(spec, interrupt_signal)
    }

    // the first command must be "init"
    let mut trigger: Trigger = {
        let signal = commands.clone();
        let init_cmd = commands
            .wait_for(|c| matches!(c, SourceCmd::SetTrigger(_)))
            .await
            .expect("watch channel must stay open during run_source");

        // cloning required to borrow opt as mut below
        match init_cmd.clone() {
            SourceCmd::SetTrigger(mut opt) => {
                init_trigger(&mut opt, signal).with_context(|| format!("init_trigger failed for {source_name}"))?
            }
            _ => unreachable!(),
        }
    };

    // Store measurements in this buffer, and replace it every `flush_rounds` rounds.
    // For now, we don't know how many measurements the source will produce, so we allocate 1 per round.
    let mut buffer = MeasurementBuffer::with_capacity(trigger.config.flush_rounds);

    // main loop
    let mut i = 1usize;
    'run: loop {
        // Wait for the trigger. It can return for two reasons:
        // - "normal case": the underlying mechanism (e.g. timer) triggers <- this is the most likely case
        // - "interrupt case": the underlying mechanism was idle (e.g. sleeping) but a new command arrived
        let reason = trigger.next().await.with_context(|| source_name.clone())?;

        let update = match reason {
            TriggerReason::Triggered => {
                // poll the source
                let timestamp = Timestamp::now();
                match source.poll(&mut buffer.as_accumulator(), timestamp) {
                    Ok(()) => (),
                    Err(PollError::CanRetry(e)) => {
                        log::error!("Non-fatal error when polling {source_name} (will retry): {e:#}");
                    }
                    Err(PollError::Fatal(e)) => {
                        log::error!("Fatal error when polling {source_name} (will stop running): {e:?}");
                        return Err(e.context(format!("fatal error when polling {source_name}")));
                    }
                };

                // Flush the measurements, not on every round for performance reasons.
                // This is done _after_ polling, to ensure that we poll at least once before flushing, even if flush_rounds is 1.
                if i % trigger.config.flush_rounds == 0 {
                    // flush and create a new buffer

                    // Hint for the new buffer capacity, great if the number of measurements per flush doesn't change much,
                    // which is often the case.
                    let prev_length = buffer.len();

                    buffer = match tx.try_send(buffer) {
                        Ok(()) => {
                            // buffer has been sent, create a new one
                            log::debug!("{source_name} flushed {prev_length} measurements");
                            MeasurementBuffer::with_capacity(prev_length)
                        }
                        Err(TrySendError::Closed(_buf)) => {
                            // the channel Receiver has been closed
                            panic!("source channel should stay open");
                        }
                        Err(TrySendError::Full(_buf)) => {
                            // the channel's buffer is full! reduce the measurement frequency
                            // TODO it would be better to choose which source to slow down based
                            // on its frequency and number of measurements per poll.
                            // buf
                            todo!("buffer is full")
                        }
                    };
                }

                // only update on some rounds, for performance reasons.
                let update = (i % trigger.config.update_rounds) == 0;

                // increase i
                i = i.wrapping_add(1);

                update
            }
            TriggerReason::Interrupted => {
                // interrupted because of a new command, forcibly update the command (see below)
                true
            }
        };

        if update {
            // update state based on the latest command
            let current_command: Option<SourceCmd> = {
                // restrict the scope of cmd_ref, otherwise it causes lifetime problems
                let cmd_ref = commands.borrow_and_update();
                if cmd_ref.has_changed() {
                    Some(cmd_ref.clone())
                } else {
                    None
                }
            };

            if let Some(cmd) = current_command {
                let mut cmd = cmd;
                let mut paused = false;
                'pause: loop {
                    log::trace!("{source_name} received {cmd:?}");
                    match cmd {
                        SourceCmd::Run => break 'pause,
                        SourceCmd::Pause => paused = true,
                        SourceCmd::Stop => {
                            // flush now, then stop
                            if !buffer.is_empty() {
                                tx.try_send(buffer)
                                    .expect("failed to flush measurements after receiving SourceCmd::Stop");
                            }
                            break 'run;
                        }
                        SourceCmd::SetTrigger(mut opt) => {
                            let prev_flush_rounds = trigger.config.flush_rounds;

                            // update the trigger
                            let signal = commands.clone();
                            trigger = init_trigger(&mut opt, signal).unwrap();

                            // don't reset the round count
                            // i = 1;

                            // estimate the required buffer capacity and allocate it
                            let prev_length = buffer.len();
                            let remaining_rounds = trigger.config.flush_rounds;
                            let hint_additional_elems = remaining_rounds * prev_length / prev_flush_rounds;
                            buffer.reserve(hint_additional_elems);

                            // don't be stuck here
                            if !paused {
                                break 'pause;
                            }
                        }
                    }
                    commands
                        .changed()
                        .await
                        .expect("command channel of paused source should remain open");
                    cmd = commands.borrow().clone();
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum TransformCmd {
    Enable,
    Disable,
}

async fn run_transforms(
    mut transforms: Vec<ConfiguredTransform>,
    mut rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<OutputMsg>,
    active_flags: Arc<AtomicU64>,
) -> anyhow::Result<()> {
    loop {
        if let Some(mut measurements) = rx.recv().await {
            // Update the list of active transforms (the PipelineController can update the flags).
            let current_flags = active_flags.load(Ordering::Relaxed);

            // Run the enabled transforms. If one of them fails, the ability to continue running depends on the error type.
            for (i, t) in &mut transforms.iter_mut().enumerate() {
                let t_flag = 1 << i;
                if current_flags & t_flag != 0 {
                    match t.transform.apply(&mut measurements) {
                        Ok(()) => (),
                        Err(TransformError::UnexpectedInput(e)) => {
                            log::error!("Transform function {} received unexpected measurements: {e:#}", t.name);
                        }
                        Err(TransformError::Fatal(e)) => {
                            log::error!(
                                "Fatal error in transform {} (this breaks the transform task!): {e:?}",
                                t.name
                            );
                            return Err(e.context(format!("fatal error in transform {}", t.name)));
                        }
                    }
                }
            }

            // Send the results to the outputs.
            tx.send(OutputMsg::WriteMeasurements(measurements))
                .context("could not send the measurements from transforms to the outputs")?;
        } else {
            log::debug!("The channel connected to the transform step has been closed, the transforms will stop.");
            break;
        }
    }
    Ok(())
}

/// A command for an output.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    output_name: String,
    mut output: Box<dyn Output>,
    mut rx: broadcast::Receiver<OutputMsg>,
    mut commands: watch::Receiver<OutputCmd>,
    mut ctx: OutputContext,
) -> anyhow::Result<()> {
    // Two possible designs:
    // A) Use one mpsc channel + one shared variable that contains the current command,
    // - when a message is received, check the command and act accordingly
    // - to change the command, update the variable and send a special message through the channel
    // In this alternative design, each Output would have one mpsc channel, and the Transform step would call send() or try_send() on each of them.
    //
    // B) use a broadcast + watch, where the broadcast discards old values when a receiver (output) lags behind,
    // instead of either (with option A):
    // - preventing the transform from running (mpsc channel's send() blocks when the queue is full).
    // - losing the most recent messages in transform, for one output. Other outputs that are not lagging behind will receive all messages fine, since try_send() does not block.
    //     The problem is: what to do with messages that could not be sent, when try_send() fails?
    //
    // We have chosen option (B).

    async fn handle_message(
        received_msg: OutputMsg,
        output_name: &str,
        output: &mut dyn Output,
        ctx: &mut OutputContext,
    ) -> anyhow::Result<()> {
        match received_msg {
            OutputMsg::WriteMeasurements(measurements) => {
                // output.write() is blocking, do it in a dedicated thread.

                // Output is not Sync, we could move the value to the future and back (idem for ctx),
                // but that would likely introduce a needless copy, and would be cumbersome to work with.
                // Instead, we use the `scoped` module.
                let res =
                    scoped::spawn_blocking_with_output(output, ctx, move |out, ctx| out.write(&measurements, ctx))
                        .await;
                match res {
                    Ok(write_res) => {
                        match write_res {
                            Ok(_) => Ok(()),
                            Err(WriteError::CanRetry(e)) => {
                                log::error!("Non-fatal error in output {output_name} (in a future version of Alumet, this means that the Output will try to write the same measurements later): {e:#}");
                                // TODO retry with the same measurements
                                Ok(())
                            }
                            Err(WriteError::Fatal(e)) => {
                                log::error!("Fatal error in output {output_name} (it will stop running): {e:?}");
                                Err(e.context(format!("fatal error in output {output_name}")))
                            }
                        }
                    }
                    Err(await_err) => {
                        if await_err.is_panic() {
                            Err(anyhow!(
                                "A blocking writing task panicked, there is a bug somewhere! Details: {}",
                                await_err
                            ))
                        } else {
                            todo!("unhandled error");
                        }
                    }
                }
            }
            OutputMsg::RegisterMetrics {
                metrics,
                source_name,
                reply_to,
            } => {
                let metric_ids = ctx.metrics.extend_infallible(metrics, &source_name);
                reply_to.send(metric_ids).await?;
                Ok(())
            }
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
                            Ok(new_cmd) => {
                                log::trace!("{output_name} received {new_cmd:?}");
                                match *new_cmd {
                                    OutputCmd::Run => (), // exit the wait
                                    OutputCmd::Stop => break, // stop the loop,
                                    OutputCmd::Pause => unreachable!(),
                                }
                            },
                            Err(_) => todo!("watch channel closed"),
                        };
                    },
                    Ok(OutputCmd::Stop) => {
                        log::trace!("{output_name} received OutputCmd::Stop");
                        break // stop the loop
                    },
                    Err(_) => todo!("watch channel closed")
                }
            },
            received_msg = rx.recv() => {
                match received_msg {
                    Ok(msg) => {
                        handle_message(msg, &output_name, output.as_mut(), &mut ctx).await?;
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("Output {output_name} is too slow, it lost the oldest {n} messages.");
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

#[derive(Debug)]
pub struct PipelineError {
    pub error: anyhow::Error,
    pub element: ElementType,
}

impl From<PollError> for PipelineError {
    fn from(value: PollError) -> Self {
        Self {
            error: match value {
                PollError::Fatal(err) => err,
                PollError::CanRetry(err) => err,
            },
            element: ElementType::Source,
        }
    }
}

impl From<TransformError> for PipelineError {
    fn from(value: TransformError) -> Self {
        Self {
            error: match value {
                TransformError::Fatal(err) => err,
                TransformError::UnexpectedInput(err) => err,
            },
            element: ElementType::Transform,
        }
    }
}

impl From<WriteError> for PipelineError {
    fn from(value: WriteError) -> Self {
        Self {
            error: match value {
                WriteError::Fatal(err) => err,
                WriteError::CanRetry(err) => err,
            },
            element: ElementType::Output,
        }
    }
}

/// Task that controls the pipeline.
///
/// It handles [`ControlMessage`]s received by `message_rx`, as well as the shutdown
/// of the pipeline triggered by `global_shutdown_recv` or [`tokio::signal::ctrl_c`].
///
/// ## Pipeline shutdown
///
/// On shutdown, the `state` is dropped, including the `watch::Sender`s
/// of the commands. Thus, `watch::Receiver::changed()` will return an error.
/// That is why the pipeline control task sends `SourceCmd::Stop` to every source,
/// and wait for the sources to terminate.
async fn pipeline_control_task(
    mut global_shutdown_recv: UnboundedReceiver<()>,
    mut message_rx: mpsc::Receiver<ControlMessage>,
    mut state: PipelineControllerState,
) {
    // Function for handling errors in tasks.
    fn handle_task_result(type_str: &str, result: Result<anyhow::Result<()>, JoinError>) {
        match result {
            Ok(Ok(())) => (), // task completed successfully
            Ok(Err(err)) => {
                // task completed with error
                log::error!("A {type_str} task in the measurement pipeline returned an error: {err:?}");
            }
            Err(err) => {
                // task panicked or was cancelled
                if err.is_panic() {
                    log::error!("A {type_str} task in the pipeline has panicked! {err:?}");
                } else if err.is_cancelled() {
                    log::error!("A {type_str} task in the pipeline has been unexpectedly cancelled. {err:?}");
                }
            }
        }
    }

    async fn join_next_source(
        source_set: &mut JoinSet<anyhow::Result<()>>,
    ) -> Option<Result<anyhow::Result<()>, JoinError>> {
        match timeout(Duration::from_secs(3), source_set.join_next()).await {
            Ok(res) => res,
            Err(_) => {
                log::error!("Timeout expired: sources did not stop on time.\nThis may be a bug in a plugin's autonomous source.");
                panic!("Timeout expired: sources did not stop on time");
            }
        }
    }

    // Pipeline control loop.
    loop {
        tokio::select! {
            biased; // no need for fairness/randomness here

            res = tokio::signal::ctrl_c() => {
                // Graceful shutdown on Ctrl+C (SIGTERM).
                res.expect("failed to listen for signal event");
                log::info!("Termination signal received, shutting down...");
                break;
            },
            _ = global_shutdown_recv.recv() => {
                // Graceful shutdown on shutdown order.
                log::debug!("Internal shutdown order received, shutting down...");
                break;
            }
            incoming_message = message_rx.recv() => {
                if let Some(message) = incoming_message {
                    // New message received
                    handle_control_message(&mut state, message);
                } else {
                    // Channel closed, shut down.
                    break;
                }
            }
        }
    }
    // End of the loop = shutdown phase.
    // At this point we no longer accept new messages.

    let mut join_sets: ElementJoinSets = state.modifier.join_sets;
    let source_command_senders: Vec<watch::Sender<SourceCmd>> = state
        .source_command_senders_by_plugin
        .values()
        .flatten()
        .cloned()
        .collect();
    let output_command_senders: Vec<watch::Sender<OutputCmd>> = state
        .output_command_senders_by_plugin
        .values()
        .flatten()
        .cloned()
        .collect();

    // Stop the sources first, and wait for them to send their last measurements to the transforms.
    log::debug!("Stopping sources...");
    for source_cs in &source_command_senders {
        source_cs.send_replace(SourceCmd::Stop);
    }
    state.autonomous_shutdown_token.cancel();
    while let Some(task_res) = join_next_source(&mut join_sets.source_set).await {
        handle_task_result("source", task_res);
    }

    // Ensure that all the `channel::Sender` that are connected to the transform task are dropped.
    // Note that autonomous sources have to take care of that themselves (but the automatic drop at the end of the task should be enough).
    drop(source_command_senders);
    drop(state.modifier.in_tx);

    // The transform task will stop because the sending half of the channel is now closed.
    // Stop the transforms, and wait for them to send their last measurements to the outputs.
    log::debug!("Waiting for transforms...");
    while let Some(task_res) = join_sets.transform_set.join_next().await {
        handle_task_result("transform", task_res);
    }

    // Stop the outputs, and wait for them to write their last measurements.
    log::debug!("Stopping outputs...");
    for output_cs in &output_command_senders {
        output_cs.send_replace(OutputCmd::Stop);
    }
    while let Some(task_res) = join_sets.output_set.join_next().await {
        handle_task_result("output", task_res);
    }
}

/// Processes a message received by the PipelineController.
///
/// This function uses the `state` to modify the pipeline according to the `message`.
fn handle_control_message(state: &mut PipelineControllerState, message: ControlMessage) {
    match message {
        ControlMessage::Shutdown => {
            state
                .global_shutdown_send
                .send(())
                .expect("failed to send shutdown message");
        }
        ControlMessage::AddSource {
            requested_name,
            plugin_name: plugin,
            source,
            trigger,
        } => {
            log::debug!("Adding new source {requested_name}");

            // prepare the source name, channels, etc.
            let modif = &mut state.modifier;
            let source_name = modif.namegen.deduplicate(format!("{plugin}/{requested_name}"), false);
            let in_tx = modif.in_tx.clone();
            let (command_tx, command_rx) = watch::channel(SourceCmd::SetTrigger(Some(trigger)));

            // save the command sender so that we can control the source task
            state
                .source_command_senders_by_plugin
                .entry(plugin)
                .or_default()
                .push(command_tx);

            // submit the task to the tokio Runtime, unless we are shutting down
            let task = run_source(source_name, source, in_tx, command_rx);
            modif.join_sets.source_set.spawn_on(task, &modif.rt_normal);
        }

        ControlMessage::ModifySource(ElementCommand {
            destination,
            command: message,
        }) => match destination {
            MessageDestination::Plugin(plugin) => {
                for s in state.source_command_senders_by_plugin.get(&plugin).unwrap() {
                    s.send(message.clone()).unwrap();
                }
            }
            MessageDestination::All => {
                for senders in state.source_command_senders_by_plugin.values() {
                    for s in senders {
                        s.send(message.clone()).unwrap();
                    }
                }
            }
        },

        ControlMessage::ModifyOutput(ElementCommand { destination, command }) => match destination {
            MessageDestination::Plugin(plugin) => {
                for s in state.output_command_senders_by_plugin.get(&plugin).unwrap() {
                    s.send(command.clone()).unwrap();
                }
            }
            MessageDestination::All => {
                for senders in state.output_command_senders_by_plugin.values() {
                    for s in senders {
                        s.send(command.clone()).unwrap();
                    }
                }
            }
        },

        ControlMessage::ModifyTransform(ElementCommand { destination, command }) => {
            let mask: u64 = match destination {
                MessageDestination::All => u64::MAX,
                MessageDestination::Plugin(plugin) => *state.transforms_mask_by_plugin.get(&plugin).unwrap(),
            };
            match command {
                TransformCmd::Enable => state.active_transforms.fetch_or(mask, Ordering::Relaxed),
                TransformCmd::Disable => state.active_transforms.fetch_nand(mask, Ordering::Relaxed),
            };
        }
    }
}

impl RunningPipeline {
    /// Blocks the current thread until all tasks in the pipeline finish.
    ///
    /// If a task returns an error or panicks, `wait_for_all` returns an error without waiting
    /// for the other tasks.
    pub fn wait_for_shutdown(mut self) -> anyhow::Result<()> {
        let handle = self.shutdown_task_handle.take().unwrap(); // cannot be called twice, unwrap should never panic
        let shutdown_res = self._rt_normal.block_on(async { handle.await });
        match shutdown_res {
            Ok(_) => Ok(()),
            Err(err) => {
                // task panicked or was cancelled
                if err.is_panic() {
                    log::error!("The shutdown task has panicked! {err:#}");
                } else if err.is_cancelled() {
                    log::error!("The shutdown task has been unexpectedly cancelled. {err:#}");
                }
                Err(err.into())
            }
        }
    }

    /// Returns a [`ControlHandle`], which allows to change the configuration
    /// of the pipeline while it is running.
    pub fn control_handle(&mut self) -> ControlHandle {
        self.control_handle.clone()
    }
}

impl Drop for RunningPipeline {
    fn drop(&mut self) {
        log::debug!("Dropping the pipeline...");
    }
}

impl ControlHandle {
    pub fn all(&self) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            destination: MessageDestination::All,
        }
    }
    pub fn plugin(&self, plugin_name: impl Into<String>) -> ScopedControlHandle {
        ScopedControlHandle {
            handle: self,
            destination: MessageDestination::Plugin(plugin_name.into()),
        }
    }

    pub fn blocking_all(&self) -> BlockingScopedControlHandle {
        BlockingScopedControlHandle {
            handle: self,
            destination: MessageDestination::All,
        }
    }

    pub fn blocking_plugin(&self, plugin_name: impl Into<String>) -> BlockingScopedControlHandle {
        BlockingScopedControlHandle {
            handle: self,
            destination: MessageDestination::Plugin(plugin_name.into()),
        }
    }

    /// Requests the pipeline to shut down.
    pub fn shutdown(&self) {
        match self.tx.try_send(ControlMessage::Shutdown) {
            Ok(_) => {}
            Err(TrySendError::Closed(_)) => {
                // This may occur when the pipeline has already shut down. It's okay.
                log::debug!("ControlHandle::shutdown() has been called but the pipeline is already shutting down.")
            }
            Err(TrySendError::Full(_)) => {
                // TODO: handle this case? tx.blocking_send(ControlMessage::Shutdown) could work
                todo!("buffer is full, cannot send command")
            }
        }
    }

    /// Adds a new source to the pipeline, without interrupting the elements
    /// (sources, transforms, outputs) that are currently running.
    pub fn add_source(&self, plugin_name: String, source_name: String, source: Box<dyn Source>, trigger: TriggerSpec) {
        let msg = ControlMessage::AddSource {
            requested_name: source_name,
            plugin_name,
            source,
            trigger,
        };
        self.tx.try_send(msg).unwrap()
    }
}

pub struct ScopedControlHandle<'a> {
    handle: &'a ControlHandle,
    destination: MessageDestination,
}
impl<'a> ScopedControlHandle<'a> {
    pub async fn control_sources(self, command: SourceCmd) {
        // TODO investigate using try_send instead of send here
        self.handle
            .tx
            .send(ControlMessage::ModifySource(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
    pub async fn control_transforms(self, command: TransformCmd) {
        self.handle
            .tx
            .send(ControlMessage::ModifyTransform(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
    pub async fn control_outputs(self, command: OutputCmd) {
        self.handle
            .tx
            .send(ControlMessage::ModifyOutput(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .await
            .unwrap();
    }
}

pub struct BlockingScopedControlHandle<'a> {
    handle: &'a ControlHandle,
    destination: MessageDestination,
}
impl<'a> BlockingScopedControlHandle<'a> {
    pub fn control_sources(self, command: SourceCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifySource(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .unwrap();
    }
    pub fn control_transforms(self, command: TransformCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifyTransform(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
            .unwrap();
    }
    pub fn control_outputs(self, command: OutputCmd) {
        self.handle
            .tx
            .blocking_send(ControlMessage::ModifyOutput(ElementCommand {
                destination: self.destination.clone(),
                command,
            }))
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
        time::Duration,
    };

    use tokio::{
        runtime::Runtime,
        sync::{broadcast, mpsc, watch},
    };

    use crate::{
        measurement::{
            MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementType,
            WrappedMeasurementValue,
        },
        metrics::{MetricRegistry, RawMetricId},
        pipeline::{builder::ConfiguredTransform, trigger::TriggerSpec, OutputContext, Transform},
        resources::{Resource, ResourceConsumer},
    };

    use super::{
        super::trigger, run_output_from_broadcast, run_source, run_transforms, OutputCmd, OutputMsg, SourceCmd,
    };

    #[test]
    fn source_triggered_by_time_normal() {
        run_source_trigger_test(false);
    }
    #[test]
    fn source_triggered_by_time_with_interruption() {
        run_source_trigger_test(true);
    }

    fn run_source_trigger_test(with_interruption: bool) {
        let rt = new_rt(2);
        let source = TestSource::new();

        let period = Duration::from_millis(10);
        let flush_rounds = 3;
        let tp = new_trigger(with_interruption, period, flush_rounds);
        println!("trigger: {tp:?}");

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

                    // Check that the correct number of rounds have passed.
                    assert_eq!(
                        measurements.len(),
                        flush_rounds,
                        "incorrect number of measurements: {measurements:?}"
                    );
                    n_polls += flush_rounds;

                    let last_point = measurements.iter().last().unwrap();
                    let last_point_value = match last_point.value {
                        WrappedMeasurementValue::U64(n) => n as usize,
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
        rt.spawn(run_source(String::from("test_source"), Box::new(source), tx, cmd_rx));
        sleep(2 * period);

        // pause source
        cmd_tx.send(SourceCmd::Pause).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(period); // some tolerance (wait for flushing)
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is paused
        sleep(2 * period);

        // still paused after SetTrigger
        cmd_tx
            .send(SourceCmd::SetTrigger(Some(new_trigger(
                with_interruption,
                period,
                flush_rounds,
            ))))
            .unwrap();
        sleep(2 * period);

        // resume source
        cmd_tx.send(SourceCmd::Run).unwrap();
        stopped.store(TestSourceState::Running as _, Ordering::Relaxed);
        sleep(period / 2); // lower tolerance (no flushing, just waiting for changes on the watch channel)

        // poll for some time
        sleep(period);

        // still running after SetTrigger
        cmd_tx
            .send(SourceCmd::SetTrigger(Some(new_trigger(
                with_interruption,
                period,
                flush_rounds,
            ))))
            .unwrap();
        sleep(2 * period);

        // stop source
        cmd_tx.send(SourceCmd::Stop).unwrap();
        stopped.store(TestSourceState::Stopping as _, Ordering::Relaxed);
        sleep(period); // some tolerance
        stopped.store(TestSourceState::Stopped as _, Ordering::Relaxed);

        // check that the source is stopped
        sleep(2 * period);

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
        let transforms: Vec<ConfiguredTransform> = transforms
            .into_iter()
            .map(|t| ConfiguredTransform {
                transform: t,
                name: String::from("test_transform"),
                plugin_name: String::from(""),
            })
            .collect();

        // create source
        let source = TestSource::new();
        let tp = new_trigger(false, Duration::from_millis(10), 2);
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
        rt.spawn(run_source(
            String::from("test_source"),
            Box::new(source),
            src_tx,
            src_cmd_rx,
        ));
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
        let tp = new_trigger(false, Duration::from_millis(10), 2);
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
        rt.spawn(run_output_from_broadcast(
            String::from("test_output"),
            output,
            out_rx,
            out_cmd_rx,
            out_ctx,
        ));
        rt.spawn(run_transforms(transforms, trans_rx, trans_tx, active_flags));
        rt.spawn(run_source(String::from("test_source"), source, src_tx, src_cmd_rx));

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

    fn new_trigger(test_interrupt: bool, period: Duration, flush_rounds: usize) -> TriggerSpec {
        let mut builder = trigger::builder::time_interval(period)
            .flush_rounds(flush_rounds)
            .update_rounds(flush_rounds);

        if test_interrupt {
            builder = builder.update_interval(Duration::from_millis(1));
        }
        builder.build().unwrap()
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
            into: &mut MeasurementAccumulator,
            timestamp: Timestamp,
        ) -> Result<(), crate::pipeline::PollError> {
            self.n_calls += 1;
            let point = MeasurementPoint::new_untyped(
                timestamp,
                RawMetricId(1),
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
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
                assert_eq!(m.resource, Resource::LocalMachine);
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
        fn write(
            &mut self,
            measurements: &MeasurementBuffer,
            _ctx: &OutputContext,
        ) -> Result<(), crate::pipeline::WriteError> {
            assert_eq!(measurements.len(), self.expected_input_len);
            self.output_count.fetch_add(measurements.len() as _, Ordering::Relaxed);
            Ok(())
        }
    }
}
