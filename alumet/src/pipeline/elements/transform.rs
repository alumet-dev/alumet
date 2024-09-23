//! Implementation and control of transform tasks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use builder::BuildContext;
use tokio::task::JoinError;
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use super::error::TransformError;
use crate::pipeline::util::matching::TransformSelector;
use crate::pipeline::util::naming::{NameGenerator, TransformName};
use crate::pipeline::PluginName;
use crate::{measurement::MeasurementBuffer, metrics::MetricRegistry, pipeline::registry::MetricReader};

/// Transforms measurements (arbitrary transformation).
pub trait Transform: Send {
    /// Applies the transform function on the measurements.
    ///
    /// After `apply` is done, the buffer is passed to the next transform, if there is one,
    /// or to the outputs.
    ///
    /// # Transforming measurements
    /// The transform is free to manipulate the measurement buffer how it sees fit.
    /// The `apply` method can:
    /// - remove some or all measurements
    /// - add new measurements
    /// - modify the measurement points
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

impl TransformControl {
    pub fn empty() -> Self {
        Self { tasks: None }
    }

    pub fn with_transforms(
        transforms: Vec<(PluginName, Box<dyn builder::TransformBuilder>)>,
        metrics: MetricReader,
        rx: mpsc::Receiver<MeasurementBuffer>,
        tx: broadcast::Sender<MeasurementBuffer>,
        rt_normal: &runtime::Handle,
    ) -> anyhow::Result<Self> {
        let built: anyhow::Result<Vec<builder::TransformRegistration>> = {
            let metrics_r = metrics.blocking_read();
            let mut namegen = NameGenerator::new();
            transforms
                .into_iter()
                .map(|(plugin, builder)| {
                    let mut ctx = BuildContext {
                        metrics: &metrics_r,
                        namegen: namegen.plugin_namespace(&plugin),
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
        transforms: Vec<builder::TransformRegistration>,
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

pub mod builder {
    use crate::{
        metrics::{Metric, MetricRegistry, RawMetricId},
        pipeline::util::naming::{PluginElementNamespace, TransformName},
    };

    /// Trait for transform builders.
    ///
    ///  # Example
    /// ```
    /// use alumet::pipeline::elements::transform::builder::{TransformBuilder, TransformRegistration, TransformBuildContext};
    /// use alumet::pipeline::{trigger, Transform};
    ///
    /// fn build_my_transform() -> anyhow::Result<Box<dyn Transform>> {
    ///     todo!("build a new transform")
    /// }
    ///
    /// let builder: &dyn TransformBuilder = &|ctx: &mut dyn TransformBuildContext| {
    ///     let transform = build_my_transform()?;
    ///     Ok(TransformRegistration {
    ///         name: ctx.transform_name("my-transform"),
    ///         transform,
    ///     })
    /// };
    /// ```
    pub trait TransformBuilder:
        FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<TransformRegistration>
    {
    }
    impl<F> TransformBuilder for F where F: FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<TransformRegistration> {}

    /// Information required to register a new transform to the measurement pipeline.
    pub struct TransformRegistration {
        pub name: TransformName,
        pub transform: Box<dyn super::Transform>,
    }

    pub(super) struct BuildContext<'a> {
        pub(super) metrics: &'a MetricRegistry,
        pub(super) namegen: &'a mut PluginElementNamespace,
    }

    /// Context accessible when building a transform.
    pub trait TransformBuildContext {
        /// Retrieves a metric by its name.
        fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
        /// Generates a name for the transform.
        fn transform_name(&mut self, name: &str) -> TransformName;
    }

    impl TransformBuildContext for BuildContext<'_> {
        fn metric_by_name(&self, name: &str) -> Option<(crate::metrics::RawMetricId, &crate::metrics::Metric)> {
            self.metrics.by_name(name)
        }

        fn transform_name(&mut self, name: &str) -> TransformName {
            TransformName(self.namegen.insert_deduplicate(name))
        }
    }
}

/// A control message for transforms.
#[derive(Debug)]
pub struct ControlMessage {
    /// Which transform(s) to reconfigure.
    pub selector: TransformSelector,
    /// The new state to apply to the selected transform(s).
    pub new_state: TaskState,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TaskState {
    Enabled,
    Disabled,
}

async fn run_all_in_order(
    mut transforms: Vec<builder::TransformRegistration>,
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
                    let builder::TransformRegistration { name, transform } = t;
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
