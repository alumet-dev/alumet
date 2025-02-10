use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};
use tokio::sync::Notify;

use super::TaskState;
use crate::pipeline::util::stream::{SharedStreamState, StreamState};

pub enum SingleOutputController {
    Blocking(Arc<SharedOutputConfig>),
    Async(Arc<SharedStreamState>),
}

pub struct SharedOutputConfig {
    pub change_notifier: Notify,
    pub atomic_state: AtomicU8,
}

impl SharedOutputConfig {
    pub fn new() -> Self {
        Self {
            change_notifier: Notify::new(),
            atomic_state: AtomicU8::new(TaskState::Run as u8),
        }
    }

    pub fn set_state(&self, state: TaskState) {
        self.atomic_state.store(state as u8, Ordering::Relaxed);
        self.change_notifier.notify_one();
    }
}

impl SingleOutputController {
    pub fn set_state(&mut self, state: TaskState) {
        match self {
            SingleOutputController::Blocking(shared) => shared.set_state(state),
            SingleOutputController::Async(arc) => arc.set(StreamState::from(state as u8)),
        }
    }
}
