use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use tokio::task::JoinError;
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::MetricReader;
use crate::pipeline::control::message::matching::TransformMatcher;
use crate::pipeline::naming::TransformName;

use super::builder::{BuildContext, TransformBuilder};
use super::run::run_all_in_order;
use super::Transform;

/// Controls the transforms of a measurement pipeline.
///
/// There can be a maximum of 64 transforms for the moment.
pub(crate) struct TransformControl {
    tasks: Option<TaskManager>,
}

struct TaskManager {
    task_handle: JoinHandle<anyhow::Result<()>>,
    active_bitset: Arc<AtomicU64>,
    names_by_bitset_position: Vec<TransformName>,
}

impl TransformControl {
    pub fn empty() -> Self {
        Self { tasks: None }
    }

    pub fn with_transforms(
        transforms: Vec<(TransformName, Box<dyn TransformBuilder>)>,
        metrics: MetricReader,
        rx: mpsc::Receiver<MeasurementBuffer>,
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: &runtime::Handle,
    ) -> anyhow::Result<Self> {
        let metrics_r = metrics.blocking_read();
        let mut built = Vec::with_capacity(transforms.len());
        for (full_name, builder) in transforms {
            let mut ctx = BuildContext { metrics: &metrics_r };
            let transform = builder(&mut ctx)
                .context("transform creation failed")
                .inspect_err(|e| log::error!("Failed to build transform {full_name}: {e:#}"))?;
            built.push((full_name, transform));
        }
        let tasks = TaskManager::spawn(built, metrics.clone(), rx, tx, rt_normal);
        Ok(Self { tasks: Some(tasks) })
    }

    pub fn handle_message(&mut self, msg: ControlMessage) -> anyhow::Result<()> {
        if let Some(tasks) = &mut self.tasks {
            tasks.reconfigure(msg);
        }
        Ok(())
    }

    pub fn has_task(&self) -> bool {
        self.tasks.is_some()
    }

    pub async fn join_next_task(&mut self) -> Result<anyhow::Result<()>, JoinError> {
        // Take the handle to avoid "JoinError: task polled after completion"
        match &mut self.tasks.take() {
            Some(tasks) => (&mut tasks.task_handle).await,
            None => panic!("join_next_task() should only be called if has_task()"),
        }
    }

    pub async fn shutdown<F>(self, handle_task_result: F)
    where
        F: Fn(Result<anyhow::Result<()>, tokio::task::JoinError>),
    {
        // Nothing to do to stop the tasks: the transform task will naturally
        // stop when the input channel is closed.

        // We simply wait for the task to finish.
        match self.tasks {
            Some(tasks) => handle_task_result(tasks.task_handle.await),
            None => (),
        }
    }
}

impl TaskManager {
    pub fn spawn(
        transforms: Vec<(TransformName, Box<dyn Transform>)>,
        metrics_r: MetricReader,
        rx: mpsc::Receiver<MeasurementBuffer>,
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: &runtime::Handle,
    ) -> Self {
        let mut active_bitset: u64 = 0;
        let mut names_by_bitset_position = Vec::with_capacity(transforms.len());

        for (i, (name, _)) in transforms.iter().enumerate() {
            active_bitset |= 1 << i;
            names_by_bitset_position.push(name.clone());
        }

        // Start the transforms task.
        let active_bitset = Arc::new(AtomicU64::new(active_bitset));
        let task = run_all_in_order(transforms, rx, tx, active_bitset.clone(), metrics_r);
        let task_handle = rt_normal.spawn(task);
        Self {
            task_handle,
            active_bitset,
            names_by_bitset_position,
        }
    }

    fn reconfigure(&mut self, msg: ControlMessage) {
        let mut bitset = self.active_bitset.load(Ordering::Relaxed);
        for (i, name) in self.names_by_bitset_position.iter().enumerate() {
            if msg.matcher.matches(name) {
                match msg.new_state {
                    TaskState::Enabled => {
                        bitset |= 1 << i;
                    }
                    TaskState::Disabled => {
                        bitset &= !(1 << i);
                    }
                }
            }
        }
        self.active_bitset.store(bitset, Ordering::Relaxed);
    }
}

/// A control message for transforms.
#[derive(Debug)]
pub struct ControlMessage {
    /// Which transform(s) to reconfigure.
    pub matcher: TransformMatcher,
    /// The new state to apply to the selected transform(s).
    pub new_state: TaskState,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TaskState {
    Enabled,
    Disabled,
}
