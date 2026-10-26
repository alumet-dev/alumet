use std::collections::{BTreeSet, HashMap};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};
use std::time::Duration;

use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::pipeline::naming::{PluginName, SourceName};

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

    /// Current polling interval of the source, if its trigger is based on a time interval.
    /// Updated whenever the trigger is replaced (see [`Reconfiguration::SetTrigger`]).
    pub poll_interval: Mutex<Option<Duration>>,
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
    poll_interval: Option<Duration>,
) -> (SingleSourceController, Arc<SharedSourceConfig>) {
    let config = Arc::new(SharedSourceConfig {
        change_notifier: Notify::new(),
        atomic_state: AtomicU8::new(initial_state as u8),
        trigger: Mutex::new(TriggerConfig::new(initial_trigger)),
        poll_interval: Mutex::new(poll_interval),
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
                        *shared.poll_interval.lock().unwrap() = new_spec.poll_interval();
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

/// Shared, queryable view of the current configuration of the managed sources.
///
/// This allows pipeline elements (such as outputs) to read the current polling
/// interval of a source, and to see the changes applied dynamically
/// (e.g. via the socket-control plugin).
#[derive(Clone, Default)]
pub struct SourceStates {
    inner: Arc<Mutex<HashMap<PluginName, Vec<Arc<SharedSourceConfig>>>>>,
}

impl SourceStates {
    pub(crate) fn add_source(&self, name: &SourceName, config: Arc<SharedSourceConfig>) {
        let plugin = PluginName(name.plugin().to_owned());
        self.inner.lock().unwrap().entry(plugin).or_default().push(config);
    }

    /// The current polling intervals of the managed sources of the given plugin, deduplicated.
    /// Empty if no source of that plugin is known, or if none is polled at a regular interval.
    pub fn poll_intervals(&self, plugin: &PluginName) -> Vec<Duration> {
        let mut intervals = BTreeSet::new();
        if let Some(configs) = self.inner.lock().unwrap().get(plugin) {
            for config in configs.iter() {
                if let Some(interval) = *config.poll_interval.lock().unwrap() {
                    intervals.insert(interval);
                }
            }
        }
        intervals.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::pipeline::elements::source::control::{Reconfiguration, TaskState};
    use crate::pipeline::naming::{PluginName, SourceName};

    use crate::pipeline::elements::source::trigger::{Trigger, TriggerSpec};
    use super::{SourceStates, new_managed};

    #[test]
    fn source_states_poll_intervals_follow_reconfiguration() {
        // TimeInterval triggers need a tokio reactor (timerfd)
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _guard = rt.enter();

        let spec = TriggerSpec::at_interval(Duration::from_secs(1));
        let trigger = Trigger::new(spec.clone()).unwrap();
        let (mut controller, config) = new_managed(trigger, TaskState::Run, spec.poll_interval());

        let states = SourceStates::default();
        let name = SourceName::from_str("my-plugin", "my-source");
        states.add_source(&name, config.clone());
        let plugin = PluginName("my-plugin".to_owned());
        assert_eq!(states.poll_intervals(&plugin), vec![Duration::from_secs(1)]);

        // simulate a dynamic reconfiguration (e.g. socket-control 'control sources set-period 500ms')
        controller.reconfigure(&Reconfiguration::SetTrigger(TriggerSpec::at_interval(
            Duration::from_millis(500),
        )));
        assert_eq!(states.poll_intervals(&plugin), vec![Duration::from_millis(500)]);
    }
}
