use std::fmt::Debug;

use anyhow::Context;
use tokio::runtime;
use tokio::sync::mpsc;
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::{MetricReader, MetricSender};
use crate::pipeline::control::message::matching::SourceMatcher;
use crate::pipeline::elements::source::run::{run_autonomous, run_managed};
use crate::pipeline::matching::SourceNamePattern;
use crate::pipeline::naming::{namespace::Namespace2, SourceName};

use super::builder;
use super::trigger::{Trigger, TriggerConstraints, TriggerSpec};

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

pub(super) enum Reconfiguration {
    SetState(TaskState),
    SetTrigger(TriggerSpec),
}

/// State of a (managed) source task.
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
#[repr(u8)]
pub(super) enum TaskState {
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
    controllers: Vec<(SourceName, super::task_controller::SingleSourceController)>,

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

    pub fn blocking_create_sources(&mut self, sources: Namespace2<builder::SourceBuilder>) -> anyhow::Result<()> {
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
            matcher: SourceMatcher::Name(SourceNamePattern::wildcard()),
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
        ctx: &mut builder::BuildContext,
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
                let (controller, config) = super::task_controller::new_managed(trigger);
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
                let controller = super::task_controller::new_autonomous(token);
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
