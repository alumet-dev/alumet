//! Implementation and control of source tasks.

use core::fmt;
use std::future::Future;
use std::pin::Pin;

use anyhow::Context;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::measurement::{MeasurementBuffer, Timestamp};
use crate::pipeline::PollError;
use crate::pipeline::Source;
use crate::pipeline::trigger::{Trigger, TriggerConstraints, TriggerReason, TriggerSpec};

use super::versioned::Versioned;

pub type AutonomousSource = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// Controls the sources of a measurement pipeline.
pub struct SourceControl {
    /// Collection of managed and autonomous source tasks.
    source_tasks: JoinSet<anyhow::Result<()>>,

    /// When cancelled, shuts the autonomous sources down.
    ///
    /// It's up to the autonomous sources to use this token properly.
    autonomous_shutdown: CancellationToken,

    /// Configurations of the managed sources.
    source_configs: Vec<(SourceName, Versioned<TaskConfig>)>,

    /// Sends measurements from Sources.
    ///
    /// This is used for creating new sources.
    in_tx: mpsc::Sender<MeasurementBuffer>,

    /// Constraints to apply to the new source triggers.
    trigger_constraints: TriggerConstraints,
}

impl SourceControl {
    fn handle_configure(&mut self, msg: ConfigureMessage) {
        let selector = msg.selector;

        // Simplifies the command and applies trigger constraints if needed.
        let command = match msg.command {
            SourceCommand::Pause => ResolvedCommand::SetState(TaskState::Pause),
            SourceCommand::Resume => ResolvedCommand::SetState(TaskState::Run),
            SourceCommand::Stop => ResolvedCommand::SetState(TaskState::Stop),
            SourceCommand::SetTrigger(mut spec) => {
                spec.constrain(&self.trigger_constraints);
                ResolvedCommand::SetTrigger(spec)
            }
        };

        for (name, source_config) in &mut self.source_configs {
            if name.matches(&selector) {
                match &command {
                    ResolvedCommand::SetState(s) => source_config.borrow_mut().state = *s,
                    ResolvedCommand::SetTrigger(spec) => {
                        let trigger = Trigger::new(spec.clone(), source_config.change_notif()).unwrap();
                        source_config.borrow_mut().new_trigger = Some(trigger);
                    }
                }
            }
        }
    }

    fn handle_create(&mut self, mut msg: CreateMessage, rt_normal: &Handle) {
        let source_name = msg.name;
        match msg.source {
            SourceSpec::Managed { source, trigger_spec } => {
                trigger_spec.constrain(&self.trigger_constraints);
                let config = Versioned::new_with_notified(|n| {
                    let trigger = Trigger::new(trigger_spec, n).unwrap();
                    TaskConfig {
                        new_trigger: Some(trigger),
                        state: TaskState::Run,
                    }
                });
                let source_task = run_managed(source_name, source, self.in_tx.clone(), config);
                // TODO make TriggerSpec tell us if it requests a higher thread priority
                self.source_tasks.spawn_on(source_task, rt_normal);
            }
            SourceSpec::Autonomous(source) => {
                let source_task = run_autonomous(source_name, source);
                self.source_tasks.spawn_on(source_task, rt_normal);
            }
        }
    }

    pub fn handle_message(&mut self, msg: ControlMessage, rt_normal: &Handle, rt_priority: &Handle) {
        match msg {
            ControlMessage::Configure(msg) => self.handle_configure(msg),
            ControlMessage::Create(msg) => self.handle_create(msg, rt_normal),
        }
    }
}

#[derive(PartialEq, Eq)]
pub struct SourceName {
    plugin: String,
    source: String,
}

pub enum ControlMessage {
    Configure(ConfigureMessage),
    Create(CreateMessage),
}

struct ConfigureMessage {
    selector: SourceSelector,
    command: SourceCommand,
}

struct CreateMessage {
    name: SourceName,
    source: SourceSpec,
}

enum SourceSpec {
    Managed {
        source: Box<dyn Source>,
        trigger_spec: TriggerSpec,
    },
    Autonomous(AutonomousSource),
}

pub enum SourceSelector {
    Single(SourceName),
    Plugin(String),
    All,
}

/// A command to send to a managed [`Source`].
pub enum SourceCommand {
    Pause,
    Resume,
    Stop,
    SetTrigger(TriggerSpec),
}

enum ResolvedCommand {
    SetState(TaskState),
    SetTrigger(TriggerSpec),
}

impl SourceName {
    pub fn matches(&self, selector: &SourceSelector) -> bool {
        match selector {
            SourceSelector::Single(full_name) => self == full_name,
            SourceSelector::Plugin(plugin_name) => &self.plugin == plugin_name,
            SourceSelector::All => true,
        }
    }
}

impl fmt::Display for SourceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.plugin, self.source)
    }
}

/// Configuration of a (managed) source task.
///
/// Can be modified while the source is running.
struct TaskConfig {
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

pub async fn run_managed(
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

    // Get the initial source configuration.
    let mut trigger = versioned_config
        .borrow_mut()
        .new_trigger
        .take()
        .expect("initial source config must contain a Trigger");

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
                    versioned_config.change_notif().await; // wait for the config to change
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
