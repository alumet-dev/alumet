//! Implementation and control of source tasks.

use std::future::Future;
use std::pin::Pin;

use anyhow::Context;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use super::error::PollError;
use crate::measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp};
use crate::metrics::MetricRegistry;
use crate::pipeline::builder::elements::SourceBuilder;
use crate::pipeline::trigger::{Trigger, TriggerConstraints, TriggerReason, TriggerSpec};
use crate::pipeline::util::naming::{NameGenerator, PluginName, ScopedNameGenerator, SourceName};
use crate::pipeline::{builder, registry};

use super::super::util::versioned::Versioned;

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
    /// Generates unique names for source tasks.
    names: NameGenerator,
    /// Read-only access to the metrics.
    metrics: registry::MetricReader,
}

struct TaskManager {
    /// Collection of managed and autonomous source tasks.
    spawned_tasks: JoinSet<anyhow::Result<()>>,

    /// Controllers for each source, by name.
    controllers: Vec<(SourceName, SingleSourceController)>,

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

struct BuildContext<'a> {
    metrics: &'a MetricRegistry,
    namegen: &'a mut ScopedNameGenerator,
}

impl SourceControl {
    pub fn new(
        trigger_constraints: TriggerConstraints,
        shutdown_token: CancellationToken,
        in_tx: mpsc::Sender<MeasurementBuffer>,
        rt_normal: runtime::Handle,
        rt_priority: runtime::Handle,
        metrics: registry::MetricReader,
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
            names: NameGenerator::new(),
            metrics,
        }
    }

    pub fn create_sources(&mut self, sources: Vec<(PluginName, SourceBuilder)>) {
        let metrics = self.metrics.blocking_read();
        for (plugin, builder) in sources {
            let mut ctx = BuildContext {
                metrics: &metrics,
                namegen: self.names.namegen_for_scope(&plugin),
            };
            self.tasks.create_source(&mut ctx, builder);
        }
    }

    pub fn create_source(&mut self, plugin: PluginName, builder: SourceBuilder) {
        let metrics = self.metrics.blocking_read();
        let mut ctx = BuildContext {
            metrics: &metrics,
            namegen: self.names.namegen_for_scope(&plugin),
        };
        self.tasks.create_source(&mut ctx, builder);
    }

    pub fn handle_message(&mut self, msg: ControlMessage) {
        match msg {
            ControlMessage::Configure(msg) => self.tasks.reconfigure(msg),
            ControlMessage::Create(msg) => self.create_source(msg.plugin, msg.builder.into()),
        }
    }
    
    pub async fn join_next_task(&mut self) -> Option<Result<anyhow::Result<()>, JoinError>> {
        self.tasks.spawned_tasks.join_next().await
    }

    pub async fn shutdown<F>(mut self, handle_task_result: F) where F: Fn(Result<anyhow::Result<()>, tokio::task::JoinError>) {
        // NOTE: self.autonomous_shutdown has already been cancelled by the parent
        // CancellationToken, therefore we don't cancel it here.
        // This cancellation has requested all the autonomous sources to stop.

        // Send a stop message to all managed sources.
        let stop_msg = ConfigureMessage {
            selector: SourceSelector::All,
            command: SourceCommand::Stop,
        };
        self.tasks.reconfigure(stop_msg);

        // Wait for managed and autonomous sources to stop.
        loop {
            match self.join_next_task().await {
                Some(res) => handle_task_result(res),
                None => break,
            }
        }

        // At the end of the method, `in_tx` is dropped,
        // which allows the channel to close when all sources finish.
    }
}

impl TaskManager {
    fn create_source(&mut self, ctx: &mut BuildContext, builder: SourceBuilder) {
        match builder {
            SourceBuilder::Managed(build) => {
                let mut reg = build(ctx);
                reg.trigger.constrain(&self.trigger_constraints);
                // TODO make TriggerSpec tell us if it requests a higher thread priority

                let mut config = Versioned::new(TaskConfig {
                    new_trigger: None,
                    state: TaskState::Pause,
                });
                let trigger = Trigger::new(reg.trigger, config.clone_unseen()).unwrap();
                config.update(|c| {
                    c.new_trigger = Some(trigger);
                    c.state = TaskState::Run
                });
                let source_task = run_managed(reg.name.clone(), reg.source, self.in_tx.clone(), config.clone_unseen());
                let controller = SingleSourceController::Managed(config);
                self.controllers.push((reg.name, controller));
                self.spawned_tasks.spawn_on(source_task, &self.rt_normal);
            }
            SourceBuilder::Autonomous(build) => {
                let token = self.shutdown_token.child_token();
                let tx = self.in_tx.clone();
                let reg = build(ctx, token.clone(), tx);

                let source_task = run_autonomous(reg.name.clone(), reg.source);
                let controller = SingleSourceController::Autonomous(token);
                self.controllers.push((reg.name, controller));
                self.spawned_tasks.spawn_on(source_task, &self.rt_normal);
            }
        };
    }

    fn reconfigure(&mut self, msg: ConfigureMessage) {
        let selector = msg.selector;

        // Simplifies the command and applies trigger constraints if needed.
        let command = match msg.command {
            SourceCommand::Pause => Reconfiguration::SetState(TaskState::Pause),
            SourceCommand::Resume => Reconfiguration::SetState(TaskState::Run),
            SourceCommand::Stop => Reconfiguration::SetState(TaskState::Stop),
            SourceCommand::SetTrigger(mut spec) => {
                spec.constrain(&self.trigger_constraints);
                Reconfiguration::SetTrigger(spec)
            }
        };

        for (name, source_controller) in &mut self.controllers {
            if selector.matches(name) {
                match source_controller {
                    SingleSourceController::Managed(source_config) => match &command {
                        Reconfiguration::SetState(s) => source_config.borrow_mut().state = *s,
                        Reconfiguration::SetTrigger(spec) => {
                            let trigger = Trigger::new(spec.clone(), source_config.clone()).unwrap();
                            source_config.borrow_mut().new_trigger = Some(trigger);
                        }
                    },
                    SingleSourceController::Autonomous(shutdown_token) => match &command {
                        Reconfiguration::SetState(TaskState::Stop) => {
                            shutdown_token.cancel();
                        }
                        _ => todo!("invalid command for autonomous source"),
                    },
                }
            }
        }
    }
}

impl builder::context::SourceBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)> {
        self.metrics.by_name(name)
    }

    fn source_name(&mut self, name: &str) -> SourceName {
        self.namegen.source_name(name)
    }
}

/// A controller for a single source.
pub(crate) enum SingleSourceController {
    /// Dynamic configuration of a managed source.
    ///
    /// This is more flexible than the token of autonomous sources.
    Managed(Versioned<TaskConfig>),

    /// When cancelled, shuts the autonomous source down.
    ///
    /// It's up to the autonomous source to use this token properly, Alumet cannot guarantee
    /// that the source will react to the cancellation (but it should!).
    Autonomous(CancellationToken),
}

pub enum ControlMessage {
    Configure(ConfigureMessage),
    Create(CreateMessage),
}

pub struct ConfigureMessage {
    pub selector: SourceSelector,
    pub command: SourceCommand,
}

pub struct CreateMessage {
    pub plugin: PluginName,
    pub builder: super::super::builder::elements::SendSourceBuilder,
}

pub enum SourceSelector {
    Single(SourceName),
    Plugin(PluginName),
    All,
}

/// A command to send to a managed [`Source`].
pub enum SourceCommand {
    Pause,
    Resume,
    Stop,
    SetTrigger(TriggerSpec),
}

enum Reconfiguration {
    SetState(TaskState),
    SetTrigger(TriggerSpec),
}

impl SourceSelector {
    pub fn matches(&self, name: &SourceName) -> bool {
        match self {
            SourceSelector::Single(full_name) => name == full_name,
            SourceSelector::Plugin(plugin_name) => name.plugin == plugin_name.0,
            SourceSelector::All => true,
        }
    }
}

/// Configuration of a (managed) source task.
///
/// Can be modified while the source is running.
pub(crate) struct TaskConfig {
    new_trigger: Option<Trigger>,
    state: TaskState,
}

/// State of a (managed) source task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
enum TaskState {
    Run,
    Pause,
    Stop,
}

pub(crate) async fn run_managed(
    source_name: SourceName,
    mut source: Box<dyn Source>,
    tx: mpsc::Sender<MeasurementBuffer>,
    mut versioned_config: Versioned<TaskConfig>,
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

    /// If the `config` contains a new [`Trigger`], replaces `trigger` with it and updates the buffer's capacity.
    fn apply_trigger_config(config: &mut TaskConfig, trigger: &mut Trigger, buffer: &mut MeasurementBuffer) {
        if let Some(t) = config.new_trigger.take() {
            let prev_flush_rounds = trigger.config.flush_rounds;

            // update the trigger
            *trigger = t;

            // estimate the required buffer capacity with this new trigger and allocate it
            let prev_length = buffer.len();
            let remaining_rounds = trigger.config.flush_rounds;
            let hint_additional_elems = remaining_rounds * prev_length / prev_flush_rounds;
            buffer.reserve(hint_additional_elems);
        }
    }

    async fn get_initial_config(versioned_config: &mut Versioned<TaskConfig>) -> anyhow::Result<Trigger> {
        let mut trigger: Option<Trigger> = None;
        loop {
            versioned_config.changed().await;
            let (new_state, new_trigger) = versioned_config
                .update_if_changed(|c| (c.state, c.new_trigger.take()))
                .unwrap();
            if new_trigger.is_some() {
                trigger = new_trigger;
            }
            match new_state {
                TaskState::Run => break,
                TaskState::Pause => continue,
                TaskState::Stop => break,
            }
        }
        trigger.context("the Trigger must be set before requesting the source to run")
    }

    // Get the initial source configuration.
    let mut trigger = get_initial_config(&mut versioned_config).await?;

    // Store measurements in this buffer, and replace it every `flush_rounds` rounds.
    // For now, we don't know how many measurements the source will produce, so we allocate 1 per round.
    let mut buffer = MeasurementBuffer::with_capacity(trigger.config.flush_rounds);

    // main loop
    let mut i = 1usize;
    'run: loop {
        // Wait for the trigger. It can return for two reasons:
        // - "normal case": the underlying mechanism (e.g. timer) triggers <- this is the most likely case
        // - "interrupt case": the underlying mechanism was idle (e.g. sleeping) but a new command arrived
        let reason = trigger.next().await.with_context(|| source_name.to_string())?;

        let mut update;
        match reason {
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
            let new_state = versioned_config.update_if_changed(|config| {
                apply_trigger_config(config, &mut trigger, &mut buffer);
                config.state
            });
            match new_state {
                None | Some(TaskState::Run) => {
                    update = false; // go back to polling
                }
                Some(TaskState::Pause) => {
                    versioned_config.changed().await; // wait for the config to change
                }
                Some(TaskState::Stop) => {
                    break 'run; // stop polling
                }
            }
        }
    }
    Ok(())
}

pub async fn run_autonomous(source_name: SourceName, source: AutonomousSource) -> anyhow::Result<()> {
    source
        .await
        .map_err(|e| e.context(format!("error in autonomous source {}", source_name)))
}
