//! Implementation and control of source tasks.

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use super::error::PollError;
use crate::measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp};
use crate::metrics::online::{MetricReader, MetricSender};
use crate::pipeline::naming::matching::SourceMatcher;
use crate::pipeline::naming::{namespace::Namespaces, SourceName};
use crate::pipeline::trigger::{Trigger, TriggerConstraints, TriggerReason, TriggerSpec};
use builder::BuildContext;

pub type AutonomousSource = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    /// Polls the source for new measurements.
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError>;
}

/// Controls the sources of a measurement pipeline.
pub(crate) struct SourceControl {
    /// Manages source tasks. Separated from `names` and `metrics` for borrow-checking reasons.
    tasks: TaskManager,
    /// Read-only and write-only access to the metrics.
    metrics: (MetricReader, MetricSender),
}

struct TaskManager {
    /// Collection of managed and autonomous source tasks.
    spawned_tasks: JoinSet<anyhow::Result<()>>,

    /// Controllers for each source, by name.
    controllers: Vec<(SourceName, task_controller::SingleSourceController)>,

    /// Cancelled when the pipeline shuts down.
    ///
    /// This token is the parent of the tokens of the autonomous sources.
    shutdown_token: CancellationToken,

    /// Constraints to apply to the new source triggers.
    trigger_constraints: TriggerConstraints,

    /// Sends measurements from Sources.
    ///
    /// This is used for creating new sources.
    /// It also keeps the transform task running.
    in_tx: mpsc::Sender<MeasurementBuffer>,

    /// Handle of the "normal" async runtime. Used for creating new sources.
    rt_normal: runtime::Handle,

    /// Handle of the "priority" async runtime. Used for creating new sources.
    rt_priority: runtime::Handle,
}

impl SourceControl {
    pub fn new(
        trigger_constraints: TriggerConstraints,
        shutdown_token: CancellationToken,
        in_tx: mpsc::Sender<MeasurementBuffer>,
        rt_normal: runtime::Handle,
        rt_priority: runtime::Handle,
        metrics: (MetricReader, MetricSender),
    ) -> Self {
        Self {
            tasks: TaskManager {
                spawned_tasks: JoinSet::new(),
                controllers: Vec::new(),
                shutdown_token,
                trigger_constraints,
                in_tx,
                rt_normal,
                rt_priority,
            },
            metrics,
        }
    }

    pub fn blocking_create_sources(&mut self, sources: Namespaces<builder::SourceBuilder>) -> anyhow::Result<()> {
        let metrics = self.metrics.0.blocking_read();
        for ((plugin, name), builder) in sources {
            let mut ctx = builder::BuildContext {
                metrics: &metrics,
                metrics_r: &self.metrics.0,
                metrics_tx: &self.metrics.1,
            };
            let full_name = SourceName::new(plugin.clone(), name);
            self.tasks
                .create_source(&mut ctx, full_name, builder)
                .inspect_err(|e| {
                    log::error!("Error in source creation requested by plugin {plugin}: {e:#}");
                })?;
            // `blocking_create_sources` is called during the startup phase. It's ok to fail early.
        }
        Ok(())
    }

    pub async fn create_sources(
        &mut self,
        builders: Vec<(SourceName, builder::SendSourceBuilder)>,
    ) -> anyhow::Result<()> {
        // We only get the lock and BuildContext once for all the sources.
        let metrics = self.metrics.0.read().await;
        let mut ctx = builder::BuildContext {
            metrics: &metrics,
            metrics_r: &self.metrics.0,
            metrics_tx: &self.metrics.1,
        };
        let n_sources = builders.len();
        log::debug!("Creating {n_sources} sources...");

        // `create_sources` is called while the pipeline is running, we want to be resilient to errors.
        // Try to build as many sources as possible, even if some fail.
        let mut n_errors = 0;
        for (name, builder) in builders {
            let _ = self
                .tasks
                .create_source(&mut ctx, name.clone(), builder.into())
                .inspect_err(|e| {
                    log::error!("Error while creating source '{name}': {e:?}");
                    n_errors += 1;
                });
        }
        if n_errors == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "failed to create {n_errors}/{n_sources} sources (see logs above)"
            ))
        }
    }

    pub async fn handle_message(&mut self, msg: ControlMessage) -> anyhow::Result<()> {
        log::trace!("handling {msg:?}");
        match msg {
            ControlMessage::Configure(msg) => self.tasks.reconfigure(msg),
            ControlMessage::CreateOne(msg) => self.create_sources(vec![(msg.name, msg.builder)]).await?,
            ControlMessage::CreateMany(msg) => self.create_sources(msg.builders).await?,
            ControlMessage::TriggerManually(msg) => self.tasks.trigger_manually(msg),
        }
        Ok(())
    }

    pub fn has_task(&self) -> bool {
        !self.tasks.spawned_tasks.is_empty()
    }

    pub async fn join_next_task(&mut self) -> Result<anyhow::Result<()>, JoinError> {
        self.tasks
            .spawned_tasks
            .join_next()
            .await
            .expect("should not be called when !has_task()")
    }

    pub async fn shutdown<F>(mut self, handle_task_result: F)
    where
        F: Fn(Result<anyhow::Result<()>, tokio::task::JoinError>),
    {
        // NOTE: self.autonomous_shutdown has already been cancelled by the parent
        // CancellationToken, therefore we don't cancel it here.
        // This cancellation has requested all the autonomous sources to stop.

        // Send a stop message to all managed sources.
        let stop_msg = ConfigureMessage {
            matcher: SourceMatcher::wildcard(),
            command: ConfigureCommand::Stop,
        };
        self.tasks.reconfigure(stop_msg);

        // Wait for managed and autonomous sources to stop.
        loop {
            match self.tasks.spawned_tasks.join_next().await {
                Some(res) => handle_task_result(res),
                None => break,
            }
        }

        // At the end of the method, `in_tx` is dropped,
        // which allows the channel to close when all sources finish.
    }
}

impl TaskManager {
    fn create_source(
        &mut self,
        ctx: &mut BuildContext,
        name: SourceName,
        builder: builder::SourceBuilder,
    ) -> anyhow::Result<()> {
        match builder {
            builder::SourceBuilder::Managed(build) => {
                // Build the source
                let mut source = build(ctx).context("managed source creation failed")?;

                // Apply constraints on the source trigger
                log::trace!("New managed source: {} with spec {:?}", name, source.trigger_spec);
                source.trigger_spec.constrain(&self.trigger_constraints);
                log::trace!("spec after constraints: {:?}", source.trigger_spec);

                // Choose the right tokio runtime (i.e. thread pool)
                let runtime = if source.trigger_spec.requests_realtime_priority() {
                    log::trace!("selected realtime runtime");
                    &self.rt_priority
                } else {
                    log::trace!("selected normal runtime");
                    &self.rt_normal
                };

                // Create the source trigger, which may be interruptible by a config change (depending on the TriggerSpec).
                // Some triggers need to be built with an executor available, therefore we use `Handle::enter()`.
                let trigger = {
                    let _guard = runtime.enter();
                    Trigger::new(source.trigger_spec).context("error in Trigger::new")?
                };
                log::trace!("new trigger created from the spec");

                // Create a controller to control the async task.
                let (controller, config) = task_controller::new_managed(trigger);
                self.controllers.push((name.clone(), controller));
                log::trace!("new controller initialized");

                // Create the future (async task).
                let source_task = run_managed(name, source.source, self.in_tx.clone(), config);
                log::trace!("source task created");

                // Spawn the future (execute the async task on the thread pool)
                self.spawned_tasks.spawn_on(source_task, runtime);
            }
            builder::SourceBuilder::Autonomous(build) => {
                let token = self.shutdown_token.child_token();
                let tx = self.in_tx.clone();
                let source = build(ctx, token.clone(), tx).context("autonomous source creation failed")?;
                log::trace!("New autonomous source: {}", name);

                let source_task = run_autonomous(name.clone(), source);
                let controller = task_controller::new_autonomous(token);
                self.controllers.push((name, controller));
                log::trace!("new controller initialized");

                self.spawned_tasks.spawn_on(source_task, &self.rt_normal);
            }
        };
        log::trace!("source task spawned on the runtime");
        Ok(())
    }

    fn reconfigure(&mut self, msg: ConfigureMessage) {
        // Simplifies the command and applies trigger constraints if needed.
        let command = match msg.command {
            ConfigureCommand::Pause => Reconfiguration::SetState(TaskState::Pause),
            ConfigureCommand::Resume => Reconfiguration::SetState(TaskState::Run),
            ConfigureCommand::Stop => Reconfiguration::SetState(TaskState::Stop),
            ConfigureCommand::SetTrigger(mut spec) => {
                spec.constrain(&self.trigger_constraints);
                Reconfiguration::SetTrigger(spec)
            }
        };

        for (name, source_controller) in &mut self.controllers {
            if msg.matcher.matches(name) {
                source_controller.reconfigure(&command);
            }
        }
    }

    fn trigger_manually(&mut self, msg: TriggerMessage) {
        let mut matches = 0;
        for (name, source_controller) in &mut self.controllers {
            if msg.matcher.matches(name) {
                matches += 1;
                source_controller.trigger_now();
            }
        }
        log::trace!("TriggerMessage matched {matches} sources.");
    }
}

pub mod builder;

/// A control message for sources.
#[derive(Debug)]
pub enum ControlMessage {
    /// Reconfigures some source(s).
    Configure(ConfigureMessage),
    /// Creates a new source.
    CreateOne(CreateOneMessage),
    /// Creates multiple sources.
    ///
    /// Sending one `CreateMany` is more efficient than sending multiple `CreateOne`.
    /// See [`crate::pipeline::control::SourceCreationBuffer`] for a high-level API that uses `CreateMany`.
    CreateMany(CreateManyMessage),
    /// Triggers some source(s).
    ///
    /// The source will be triggered "as soon as possible", i.e. when it receives the messages
    /// and processes it. Sources must be configured to accept manual trigger, otherwise this message
    /// will do nothing.
    TriggerManually(TriggerMessage),
}

#[derive(Debug)]
pub struct ConfigureMessage {
    /// Which transform(s) to reconfigure.
    pub matcher: SourceMatcher,
    pub command: ConfigureCommand,
}

#[derive(Debug)]
pub struct CreateOneMessage {
    pub name: SourceName,
    pub builder: builder::SendSourceBuilder,
}

#[derive(Debug)]
pub struct CreateManyMessage {
    pub builders: Vec<(SourceName, builder::SendSourceBuilder)>,
}

#[derive(Debug)]
pub struct TriggerMessage {
    /// Which transform(s) to trigger.
    pub matcher: SourceMatcher,
}

/// A command to send to a managed [`Source`].
#[derive(Debug)]
pub enum ConfigureCommand {
    Pause,
    Resume,
    Stop,
    SetTrigger(TriggerSpec),
}

enum Reconfiguration {
    SetState(TaskState),
    SetTrigger(TriggerSpec),
}

/// State of a (managed) source task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
#[repr(u8)]
enum TaskState {
    Run,
    Pause,
    Stop,
}

impl From<u8> for TaskState {
    fn from(value: u8) -> Self {
        const RUN: u8 = TaskState::Run as u8;
        const PAUSE: u8 = TaskState::Pause as u8;

        match value {
            RUN => TaskState::Run,
            PAUSE => TaskState::Pause,
            _ => TaskState::Stop,
        }
    }
}

mod task_controller;

pub(crate) async fn run_managed(
    source_name: SourceName,
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    config: Arc<task_controller::SharedSourceConfig>,
) -> anyhow::Result<()> {
    /// Flushes the measurement and returns a new buffer.
    fn flush(buffer: MeasurementBuffer, tx: &mpsc::Sender<MeasurementBuffer>, name: &SourceName) -> MeasurementBuffer {
        // Hint for the new buffer capacity, great if the number of measurements per flush doesn't change much,
        // which is often the case.
        let prev_length = buffer.len();

        match tx.try_send(buffer) {
            Ok(()) => {
                // buffer has been sent, create a new one
                log::debug!("{name} flushed {prev_length} measurements");
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
        }
    }

    // Estimate the required buffer capacity with the new trigger and allocate it.
    fn adapt_buffer_after_trigger_change(
        buffer: &mut MeasurementBuffer,
        prev_flush_rounds: usize,
        new_flush_rounds: usize,
    ) {
        let prev_length = buffer.len();
        let remaining_rounds = new_flush_rounds;
        let hint_additional_elems = remaining_rounds * prev_length / prev_flush_rounds;
        buffer.reserve(hint_additional_elems);
    }

    // Get the initial source configuration.
    let mut trigger = config
        .new_trigger
        .lock()
        .unwrap()
        .take()
        .expect("the Trigger must be set before starting the source");
    log::trace!("source {source_name} got initial config");

    // Store measurements in this buffer, and replace it every `flush_rounds` rounds.
    // For now, we don't know how many measurements the source will produce, so we allocate 1 per round.
    let mut buffer = MeasurementBuffer::with_capacity(trigger.config.flush_rounds);

    // This Notify is used to "interrupt" the trigger mechanism when the source configuration is modified
    // by the control loop.
    let config_change = &config.change_notifier;

    // main loop
    let mut i = 1usize;
    'run: loop {
        // Wait for the trigger. It can return for two reasons:
        // - "normal case": the underlying mechanism (e.g. timer) triggers <- this is the most likely case
        // - "interrupt case": the underlying mechanism was idle (e.g. sleeping) but a new command arrived
        let reason = trigger
            .next(config_change)
            .await
            .with_context(|| source_name.to_string())?;

        let mut update;
        match reason {
            TriggerReason::Triggered => {
                // poll the source
                let timestamp = Timestamp::now();
                match source.poll(&mut buffer.as_accumulator(), timestamp) {
                    Ok(()) => (),
                    Err(PollError::NormalStop) => {
                        log::info!("Source {source_name} stopped itself.");
                        break 'run; // stop polling
                    }
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
                    buffer = flush(buffer, &tx, &source_name);
                }

                // only update on some rounds, for performance reasons.
                update = (i % trigger.config.update_rounds) == 0;
                i = i.wrapping_add(1);
            }
            TriggerReason::Interrupted => {
                // interrupted because of a new command, forcibly update the command (see below)
                update = true;
            }
        };

        while update {
            let new_state = config.atomic_state.load(Ordering::Relaxed);
            let new_trigger = config.new_trigger.lock().unwrap().take();
            if let Some(t) = new_trigger {
                let prev_flush_rounds = trigger.config.flush_rounds;
                let new_flush_rounds = t.config.flush_rounds;
                trigger = t;
                adapt_buffer_after_trigger_change(&mut buffer, prev_flush_rounds, new_flush_rounds);
            }
            match new_state.into() {
                TaskState::Run => {
                    update = false; // go back to polling
                }
                TaskState::Pause => {
                    config_change.notified().await; // wait for the config to change
                }
                TaskState::Stop => {
                    break 'run; // stop polling
                }
            }
        }
    }

    // source stopped, flush the buffer
    if !buffer.is_empty() {
        flush(buffer, &tx, &source_name);
    }

    Ok(())
}

pub async fn run_autonomous(source_name: SourceName, source: AutonomousSource) -> anyhow::Result<()> {
    source.await.map_err(|e| {
        log::error!("Error in autonomous source {source_name} (will stop running): {e:?}");
        e.context(format!("error in autonomous source {source_name}"))
    })
}
