use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use tokio::task::{JoinError, JoinSet};
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
};

use crate::measurement::MeasurementBuffer;
use crate::metrics::online::MetricReader;
use crate::pipeline::control::matching::TransformMatcher;
use crate::pipeline::error::PipelineError;
use crate::pipeline::naming::TransformName;

use super::builder::{BuildContext, TransformBuilder};
use super::run::run_all_in_order;
use super::Transform;

/// Controls the transforms of a measurement pipeline.
///
/// There can be a maximum of 64 transforms for the moment.
pub(crate) struct TransformControl {
    tasks: TaskManager,
}

struct TaskManager {
    // Even though there is only one task, we don't use its JoinHandle directly,
    // because awaiting it consumes the task.
    spawned_tasks: JoinSet<Result<(), PipelineError>>,
    active_bitset: Arc<AtomicU64>,
    names_by_bitset_position: Vec<TransformName>,
}

impl TransformControl {
    pub fn empty() -> Self {
        Self {
            tasks: TaskManager {
                spawned_tasks: JoinSet::new(),
                active_bitset: Arc::new(AtomicU64::new(0)),
                names_by_bitset_position: Vec::new(),
            },
        }
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
        Ok(Self { tasks })
    }

    pub fn handle_message(&mut self, msg: ControlMessage) -> anyhow::Result<()> {
        self.tasks.reconfigure(msg);
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

    pub async fn shutdown<F>(mut self, mut handle_task_result: F)
    where
        F: FnMut(Result<Result<(), PipelineError>, tokio::task::JoinError>),
    {
        // Nothing to do to stop the tasks: the transform task will naturally
        // stop when the input channel is closed.

        // We simply wait for the task to finish.
        while let Some(res) = self.tasks.spawned_tasks.join_next().await {
            handle_task_result(res);
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
        let mut set = JoinSet::new();
        let active_bitset = Arc::new(AtomicU64::new(active_bitset));
        let task = run_all_in_order(transforms, rx, tx, active_bitset.clone(), metrics_r);
        set.spawn_on(task, rt_normal);
        Self {
            spawned_tasks: set,
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
        log::trace!("new 'enabled' bitset: {bitset}");
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
