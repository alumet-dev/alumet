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
