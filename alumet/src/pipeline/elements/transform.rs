//! Implementation and control of transform tasks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use tokio::task::JoinError;
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use super::error::TransformError;
use crate::pipeline::builder::elements::{TransformBuilder, TransformRegistration};
use crate::pipeline::util::matching::TransformSelector;
use crate::pipeline::util::naming::{NameGenerator, ScopedNameGenerator, TransformName};
use crate::pipeline::{builder, PluginName};
use crate::{measurement::MeasurementBuffer, metrics::MetricRegistry, pipeline::registry::MetricReader};

/// Transforms measurements.
pub trait Transform: Send {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError>;
}

/// Shared data that can be accessed by transforms.
pub struct TransformContext<'a> {
    pub metrics: &'a MetricRegistry,
}

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

struct BuildContext<'a> {
    metrics: &'a MetricRegistry,
    namegen: &'a mut ScopedNameGenerator,
}

impl TransformControl {
    pub fn empty() -> Self {
        Self { tasks: None }
    }

    pub fn with_transforms(
        transforms: Vec<(PluginName, Box<dyn TransformBuilder>)>,
        metrics: MetricReader,
        rx: mpsc::Receiver<MeasurementBuffer>,
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: &runtime::Handle,
    ) -> anyhow::Result<Self> {
        let built: anyhow::Result<Vec<TransformRegistration>> = {
            let metrics_r = metrics.blocking_read();
            let mut namegen = NameGenerator::new();
            transforms
                .into_iter()
                .map(|(plugin, builder)| {
                    let mut ctx = BuildContext {
                        metrics: &metrics_r,
                        namegen: namegen.namegen_for_scope(&plugin),
                    };
                    builder(&mut ctx)
                        .context("transform creation failed")
                        .inspect_err(|e| log::error!("Error in transform creation requested by plugin {plugin}: {e:#}"))
                })
                .collect()
        };
        let tasks = TaskManager::spawn(built?, metrics.clone(), rx, tx, rt_normal);
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
        transforms: Vec<TransformRegistration>,
        metrics_r: MetricReader,
        rx: mpsc::Receiver<MeasurementBuffer>,
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: &runtime::Handle,
    ) -> Self {
        let mut active_bitset: u64 = 0;
        let mut names_by_bitset_position = Vec::with_capacity(transforms.len());

        for (i, reg) in transforms.iter().enumerate() {
            active_bitset |= 1 << i;
            names_by_bitset_position.push(reg.name.clone());
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
            if msg.selector.matches(name) {
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

impl builder::context::TransformBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)> {
        self.metrics.by_name(name)
    }

    fn transform_name(&mut self, name: &str) -> TransformName {
        self.namegen.transform_name(name)
    }
}

#[derive(Debug)]
pub struct ControlMessage {
    pub selector: TransformSelector,
    pub new_state: TaskState,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TaskState {
    Enabled,
    Disabled,
}

async fn run_all_in_order(
    mut transforms: Vec<TransformRegistration>,
    mut rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<MeasurementBuffer>,
    active_flags: Arc<AtomicU64>,
    metrics_reader: MetricReader,
) -> anyhow::Result<()> {
    loop {
        if let Some(mut measurements) = rx.recv().await {
            // Update the list of active transforms.
            let current_flags = active_flags.load(Ordering::Relaxed);

            // Build the transform context.
            // This will block the publication of any modification to the MetricRegistry until the context is dropped.
            let metrics = &metrics_reader.read().await;
            let ctx = TransformContext { metrics };

            // Run the enabled transforms. If one of them fails, the ability to continue running depends on the error type.
            for (i, t) in &mut transforms.iter_mut().enumerate() {
                let t_flag = 1 << i;
                if current_flags & t_flag != 0 {
                    let TransformRegistration { name, transform } = t;
                    match transform.apply(&mut measurements, &ctx) {
                        Ok(()) => (),
                        Err(TransformError::UnexpectedInput(e)) => {
                            log::error!("Transform {name} received unexpected measurements: {e:#}");
                        }
                        Err(TransformError::Fatal(e)) => {
                            log::error!("Fatal error in transform {name} (this breaks the transform task!): {e:?}");
                            return Err(e.context(format!("fatal error in transform {name}")));
                        }
                    }
                }
            }

            // Send the results to the outputs.
            tx.send(measurements)
                .context("could not send the measurements from transforms to the outputs")?;
        } else {
            log::debug!("The channel connected to the transform step has been closed, the transforms will stop.");
            break;
        }
    }
    Ok(())
}
