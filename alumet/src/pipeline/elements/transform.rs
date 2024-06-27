//! Implementation and control of transform tasks.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Context;
use tokio::{
    runtime,
    sync::{broadcast, mpsc},
    task::JoinHandle,
};

use super::error::TransformError;
use crate::pipeline::util::naming::TransformName;
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
pub struct TransformControl {
    task_handle: JoinHandle<anyhow::Result<()>>,
    active_bitset: Arc<AtomicU64>,
    names_by_bitset_position: Vec<TransformName>,
}

impl TransformControl {
    pub fn create_transforms(
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

    pub fn handle_message(&mut self, msg: ControlMessage) {
        let mut bitset = self.active_bitset.load(Ordering::Relaxed);
        for (i, name) in self.names_by_bitset_position.iter().enumerate() {
            if msg.selector.matches(name) {
                match msg.new_state {
                    TransformState::Enabled => {
                        bitset |= 1 << i;
                    }
                    TransformState::Disabled => {
                        bitset &= !(1 << i);
                    }
                }
            }
        }
        self.active_bitset.store(bitset, Ordering::Relaxed);
    }

    pub fn shutdown(self) {
        // Nothing to do for the moment: the transform task will naturally
        // stop when the input channel is closed.
    }
}

pub enum TransformSelector {
    Single(TransformName),
    Plugin(String),
    All,
}

impl TransformSelector {
    pub fn matches(&self, name: &TransformName) -> bool {
        match self {
            TransformSelector::Single(full_name) => name == full_name,
            TransformSelector::Plugin(plugin_name) => &name.plugin == plugin_name,
            TransformSelector::All => true,
        }
    }
}

pub struct ControlMessage {
    selector: TransformSelector,
    new_state: TransformState,
}

pub enum TransformState {
    Enabled,
    Disabled,
}

async fn run_all_in_order(
    mut transforms: Vec<(TransformName, Box<dyn Transform>)>,
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
                    let (name, transform) = t;
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
