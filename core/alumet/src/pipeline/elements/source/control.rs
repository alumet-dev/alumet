use std::fmt::Debug;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use anyhow::Context;
use num_enum::{FromPrimitive, IntoPrimitive};
use tokio::runtime;
use tokio::sync::{Notify, mpsc};
use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::{MetricReader, MetricSender};
use crate::pipeline::control::matching::SourceMatcher;
use crate::pipeline::elements::source::builder::SourcePace;
use crate::pipeline::elements::source::run::{run_autonomous, run_managed};
use crate::pipeline::error::PipelineError;
use crate::pipeline::matching::{ElementNamePattern, SourceNamePattern};
use crate::pipeline::naming::{ElementKind, ElementName};
use crate::pipeline::naming::{SourceName, namespace::Namespace2};

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

#[derive(Debug)]
pub(super) enum Reconfiguration {
    SetState(TaskState),
    SetTrigger(TriggerSpec),
}

/// State of a (managed) source task.
#[derive(Clone, Debug, PartialEq, Eq, Copy, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum TaskState {
    Run,
    Pause,
    #[num_enum(default)]
    Stop,
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
    spawned_tasks: JoinSet<Result<(), PipelineError>>,

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
        match msg {
            ControlMessage::Configure(msg) => self.tasks.reconfigure(msg),
            ControlMessage::CreateOne(msg) => self.create_sources(vec![(msg.name, msg.builder)]).await?,
            ControlMessage::CreateMany(msg) => self.create_sources(msg.builders).await?,
            ControlMessage::TriggerManually(msg) => self.tasks.trigger_manually(msg),
        }
        Ok(())
    }

    pub async fn join_next_task(&mut self) -> Result<Result<(), PipelineError>, JoinError> {
        match self.tasks.spawned_tasks.join_next().await {
            Some(res) => res,
            None => unreachable!("join_next_task must be guarded by has_task to prevent an infinite loop"),
        }
    }

    pub fn has_task(&self) -> bool {
        !self.tasks.spawned_tasks.is_empty()
    }

    pub fn list_elements(&self, buf: &mut Vec<ElementName>, pat: &ElementNamePattern) {
        if pat.kind == None || pat.kind == Some(ElementKind::Source) {
            buf.extend(self.tasks.controllers.iter().filter_map(|(name, _)| {
                if pat.matches(name) {
                    Some(name.to_owned().into())
                } else {
                    None
                }
            }))
        }
    }

    pub async fn shutdown<F>(mut self, mut handle_task_result: F)
    where
        F: FnMut(Result<Result<(), PipelineError>, tokio::task::JoinError>),
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
        /// Spawns a task on a JoinSet.
        /// When built with tokio unstable features, give a name to the task.
        fn spawn_task<R: Send + 'static>(
            set: &mut JoinSet<R>,
            source_task: impl Future<Output = R> + Send + 'static,
            runtime: &tokio::runtime::Handle,
        ) {
            #[cfg(not(tokio_unstable))]
            {
                set.spawn_on(source_task, runtime);
            }
            #[cfg(tokio_unstable)]
            {
                // Give a proper name to the tokio's task, so that it's easier to debug (in particular with tokio-console).
                // For now, this is an unstable API of tokio.
                set.build_task()
                    .name(name.to_string().as_str())
                    .spawn_on(source_task, runtime);
            }
        }

        match builder {
            builder::SourceBuilder::Managed(build, pace) => {
                // Build the source
                let mut source = build(ctx).context("managed source creation failed")?;

                // Apply constraints on the source trigger
                log::trace!(
                    "New managed source: {} with spec {:?} and initial state {:?}",
                    name,
                    source.trigger_spec,
                    source.initial_state
                );
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

                // If the source is blocking, create a dedicated runtime that we will run on a dedicated thread.
                let dedicated_rt = if pace == SourcePace::Blocking {
                    Some(
                        tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .with_context(|| format!("failed to build dedicated runtime for {name}"))?,
                    )
                } else {
                    None
                };

                // Create the source trigger, which may be interruptible by a config change (depending on the TriggerSpec).
                // Some triggers need to be built with an executor available, therefore we use `Handle::enter()`.
                let trigger = {
                    match &dedicated_rt {
                        Some(rt) => {
                            let _guard = rt.enter();
                            Trigger::new(source.trigger_spec).context("error in Trigger::new")?
                        }
                        None => {
                            let _guard = runtime.enter();
                            Trigger::new(source.trigger_spec).context("error in Trigger::new")?
                        }
                    }
                };
                log::trace!("new trigger created from the spec: {trigger:?}");

                // Create a controller to control the async task.
                let (controller, config) = super::task_controller::new_managed(trigger, source.initial_state);
                self.controllers.push((name.clone(), controller));
                log::trace!("new controller initialized");

                // Create the future (async task).
                let source_task = run_managed(name.clone(), source.source, self.in_tx.clone(), config);
                log::trace!("source task created: {name}");

                match pace {
                    builder::SourcePace::Fast => {
                        // Spawn the future (execute the async task on the thread pool)
                        spawn_task(&mut self.spawned_tasks, source_task, runtime);
                    }
                    builder::SourcePace::Blocking => {
                        // Spawn a dedicated thread for this future.
                        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
                        let source_name = name.clone();
                        let rt = dedicated_rt.unwrap();
                        std::thread::spawn(move || {
                            let work = move || -> anyhow::Result<()> {
                                rt.block_on(source_task)?;
                                Ok(())
                            };

                            let res = match std::panic::catch_unwind(AssertUnwindSafe(|| work())) {
                                Ok(res) => res,
                                Err(panic) => Err(anyhow::anyhow!("source thread panicked: {panic:?}")),
                            };
                            let res = res.map_err(|e| PipelineError::for_element(source_name, e));
                            result_tx.send(res).expect("receiver dropped");
                        });
                        // Spawn a small task that will stop when the dedicated thread stops, ie when the source_task stops.
                        let thread_waiter = async move {
                            let res = result_rx.await.expect("sender dropped, did the thread panic?");
                            res // propagate the result to alumet control
                        };
                        spawn_task(&mut self.spawned_tasks, thread_waiter, runtime);
                    }
                }
            }
            builder::SourceBuilder::Autonomous(build) => {
                let token = self.shutdown_token.child_token();
                let tx = self.in_tx.clone();
                let source = build(ctx, token.clone(), tx).context("autonomous source creation failed")?;
                log::trace!("New autonomous source: {}", name);

                let source_task = run_autonomous(name.clone(), source);
                let controller = super::task_controller::new_autonomous(token);
                self.controllers.push((name.clone(), controller));
                log::trace!("new controller initialized");

                spawn_task(&mut self.spawned_tasks, source_task, &self.rt_normal);
            }
        };
        log::trace!("source task spawned on the runtime: {name}");
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
