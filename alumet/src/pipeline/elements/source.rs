//! Implementation and control of source tasks.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::JoinError;
use tokio_util::sync::CancellationToken;

use super::error::PollError;
use crate::measurement::{MeasurementAccumulator, MeasurementBuffer, Timestamp};
use crate::metrics::MetricRegistry;
use crate::pipeline::builder::elements::SourceBuilder;
use crate::pipeline::trigger::{Trigger, TriggerConstraints, TriggerReason, TriggerSpec};
use crate::pipeline::util::join_set::JoinSet;
use crate::pipeline::util::naming::{NameGenerator, PluginName, ScopedNameGenerator, SourceName};
use crate::pipeline::{builder, registry};

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
            ControlMessage::TriggerManually(msg) => self.tasks.trigger_manually(msg),
        }
    }

    pub async fn join_next_task(&mut self) -> Result<anyhow::Result<()>, JoinError> {
        self.tasks.spawned_tasks.join_next_completion().await
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
            selector: SourceSelector::All,
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
    fn create_source(&mut self, ctx: &mut BuildContext, builder: SourceBuilder) {
        match builder {
            SourceBuilder::Managed(build) => {
                // Build the source
                let mut reg = build(ctx);

                // Apply constraints on the source trigger
                log::trace!("New managed source: {} with spec {:?}", reg.name, reg.trigger_spec);
                reg.trigger_spec.constrain(&self.trigger_constraints);
                log::trace!("spec after constraints: {:?}", reg.trigger_spec);

                // Choose the right tokio runtime (i.e. thread pool)
                let runtime = if reg.trigger_spec.requests_realtime_priority() {
                    log::trace!("selected realtime runtime");
                    &self.rt_priority
                } else {
                    log::trace!("selected normal runtime");
                    &self.rt_normal
                };

                // Create the source trigger, which may be interruptible by a config change (depending on the TriggerSpec).
                // Some triggers need to be built on the async runtime, hence we use `block_on`.
                let trigger = runtime.block_on(async { Trigger::new(reg.trigger_spec) }).unwrap();
                log::trace!("new trigger created from the spec");

                // Create a controller to control the async task.
                let (controller, config) = task_controller::new_managed(trigger);
                self.controllers.push((reg.name.clone(), controller));
                log::trace!("new controller initialized");

                // Create the future (async task).
                let source_task = run_managed(reg.name, reg.source, self.in_tx.clone(), config);
                log::trace!("source task created");

                // Spawn the future (execute the async task on the thread pool)
                self.spawned_tasks.spawn_on(source_task, runtime);
                log::trace!("source task spawned on the runtime");
            }
            SourceBuilder::Autonomous(build) => {
                let token = self.shutdown_token.child_token();
                let tx = self.in_tx.clone();
                let reg = build(ctx, token.clone(), tx);
                log::trace!("New autonomous source: {}", reg.name);

                let source_task = run_autonomous(reg.name.clone(), reg.source);
                let controller = task_controller::new_autonomous(token);
                self.controllers.push((reg.name, controller));
                log::trace!("new controller initialized");

                self.spawned_tasks.spawn_on(source_task, &self.rt_normal);
                log::trace!("source task spawned on the runtime");
            }
        };
    }

    fn reconfigure(&mut self, msg: ConfigureMessage) {
        let selector = msg.selector;

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
            if selector.matches(name) {
                source_controller.reconfigure(&command);
            }
        }
    }

    fn trigger_manually(&mut self, msg: TriggerMessage) {
        let selector = msg.selector;
        for (name, source_controller) in &mut self.controllers {
            if selector.matches(name) {
                source_controller.trigger_now();
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

pub enum ControlMessage {
    Configure(ConfigureMessage),
    Create(CreateMessage),
    TriggerManually(TriggerMessage),
}

pub struct ConfigureMessage {
    pub selector: SourceSelector,
    pub command: ConfigureCommand,
}

pub struct CreateMessage {
    pub plugin: PluginName,
    pub builder: super::super::builder::elements::SendSourceBuilder,
}

pub struct TriggerMessage {
    pub selector: SourceSelector,
}

pub enum SourceSelector {
    Single(SourceName),
    Plugin(PluginName),
    All,
}

/// A command to send to a managed [`Source`].
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

impl SourceSelector {
    pub fn matches(&self, name: &SourceName) -> bool {
        match self {
            SourceSelector::Single(full_name) => name == full_name,
            SourceSelector::Plugin(plugin_name) => name.plugin == plugin_name.0,
            SourceSelector::All => true,
        }
    }
}

mod task_controller {
    use std::sync::{
        atomic::{AtomicU8, Ordering},
        Arc, Mutex,
    };

    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use crate::pipeline::trigger::{ManualTrigger, Trigger};

    use super::{Reconfiguration, TaskState};

    /// A controller for a single source.
    pub enum SingleSourceController {
        /// Dynamic configuration of a managed source + manual trigger.
        ///
        /// This is more flexible than the token of autonomous sources.
        Managed(Arc<SharedSourceConfig>),

        /// When cancelled, shuts the autonomous source down.
        ///
        /// It's up to the autonomous source to use this token properly, Alumet cannot guarantee
        /// that the source will react to the cancellation (but it should!).
        Autonomous(CancellationToken),
    }

    // struct SourceConfigReader(Arc<SharedSourceConfig>);

    pub struct SharedSourceConfig {
        pub change_notifier: Notify,
        pub atomic_state: AtomicU8,
        pub new_trigger: Mutex<Option<Trigger>>,
        pub manual_trigger: Option<ManualTrigger>,
    }

    pub fn new_managed(initial_trigger: Trigger) -> (SingleSourceController, Arc<SharedSourceConfig>) {
        let manual_trigger = initial_trigger.manual_trigger();
        let config = Arc::new(SharedSourceConfig {
            change_notifier: Notify::new(),
            atomic_state: AtomicU8::new(TaskState::Run as u8),
            new_trigger: Mutex::new(Some(initial_trigger)),
            manual_trigger,
        });
        (SingleSourceController::Managed(config.clone()), config)
    }

    pub fn new_autonomous(shutdown_token: CancellationToken) -> SingleSourceController {
        SingleSourceController::Autonomous(shutdown_token)
    }

    impl SingleSourceController {
        pub fn reconfigure(&mut self, command: &Reconfiguration) {
            match self {
                SingleSourceController::Managed(shared) => {
                    match &command {
                        Reconfiguration::SetState(new_state) => {
                            // TODO use a bit to signal that there's a new trigger?
                            shared.atomic_state.store(*new_state as u8, Ordering::Relaxed);
                        }
                        Reconfiguration::SetTrigger(new_spec) => {
                            let trigger = Trigger::new(new_spec.to_owned()).unwrap();
                            *shared.new_trigger.lock().unwrap() = Some(trigger);
                        }
                    }
                    shared.change_notifier.notify_one();
                }
                SingleSourceController::Autonomous(shutdown_token) => match &command {
                    Reconfiguration::SetState(TaskState::Stop) => {
                        shutdown_token.cancel();
                    }
                    _ => todo!("invalid command for autonomous source"),
                },
            }
        }

        pub fn trigger_now(&mut self) {
            match self {
                SingleSourceController::Managed(shared) => {
                    if let Some(t) = &shared.manual_trigger {
                        t.trigger_now();
                    }
                }
                _ => (),
            }
        }
    }
}

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
                        return Ok(());
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
    Ok(())
}

pub async fn run_autonomous(source_name: SourceName, source: AutonomousSource) -> anyhow::Result<()> {
    source
        .await
        .map_err(|e| e.context(format!("error in autonomous source {}", source_name)))
}
