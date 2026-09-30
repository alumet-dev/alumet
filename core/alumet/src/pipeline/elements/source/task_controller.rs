use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::control::{Reconfiguration, TaskState};
use super::trigger::{ManualTrigger, Trigger};

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

pub struct SharedSourceConfig {
    /// Our way to wake up the source and tell it that its config has changed.
    /// When notified in this way, the source loop will update its state and trigger.
    pub change_notifier: Notify,

    /// Current state of the source.
    pub atomic_state: AtomicU8,

    /// Info about the current or next trigger of the source.
    pub trigger: Mutex<TriggerConfig>,
}

pub struct TriggerConfig {
    /// The new trigger to use in [`source::run::run_managed`].
    /// Will be taken by the source loop.
    ///
    /// Is `None` when the source loop has not seen the state change yet.
    pub new_trigger: Option<Trigger>,

    /// The current handle to use for manually triggering the source.
    /// It changes every time the trigger is replaced.
    ///
    /// Is `None` when the current (or "new") trigger does not support manual trigger.
    pub manual_trigger: Option<ManualTrigger>,
}

impl TriggerConfig {
    pub fn new(new_trigger: Trigger) -> Self {
        let manual_trigger = new_trigger.manual_trigger();
        log::trace!("new manual_trigger = {manual_trigger:?}");
        Self {
            new_trigger: Some(new_trigger),
            manual_trigger,
        }
    }
}

pub fn new_managed(
    initial_trigger: Trigger,
    initial_state: TaskState,
) -> (SingleSourceController, Arc<SharedSourceConfig>) {
    let config = Arc::new(SharedSourceConfig {
        change_notifier: Notify::new(),
        atomic_state: AtomicU8::new(initial_state as u8),
        trigger: Mutex::new(TriggerConfig::new(initial_trigger)),
    });
    (SingleSourceController::Managed(config.clone()), config)
}

pub fn new_autonomous(shutdown_token: CancellationToken) -> SingleSourceController {
    SingleSourceController::Autonomous(shutdown_token)
}

impl SharedSourceConfig {
    pub fn take_new_trigger(&self) -> Option<Trigger> {
        self.trigger.lock().unwrap().new_trigger.take()
    }
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
                        // We change both the "trigger" and the "manual trigger".
                        // If the manual trigger is used before the source loop picks up the new trigger, it's okay:
                        // we use `Notify::notify_one`, which will store 1 permit, and the source will be manually
                        // triggered just after it takes the new trigger.
                        let trigger = Trigger::new(new_spec.to_owned()).unwrap();
                        *shared.trigger.lock().unwrap() = TriggerConfig::new(trigger);
                    }
                }
                log::trace!("reconfiguring source with {:p}", *shared);
                shared.change_notifier.notify_one();
            }
            SingleSourceController::Autonomous(shutdown_token) => match &command {
                Reconfiguration::SetState(TaskState::Stop) => {
                    shutdown_token.cancel();
                }
                _ => log::warn!(
                    "unsupported reconfiguration command received for autonomous source, ignoring: {command:?}"
                ),
            },
        }
    }

    pub fn trigger_now(&mut self) {
        match self {
            SingleSourceController::Managed(shared) => {
                if let Some(t) = &shared.trigger.lock().unwrap().manual_trigger {
                    t.trigger_now();
                }
            }
            SingleSourceController::Autonomous(_) => {
                log::warn!("unsupported trigger command received for autonomous source, ignoring");
            }
        }
    }
}
