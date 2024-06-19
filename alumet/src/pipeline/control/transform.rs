//! Implementation and control of transform tasks.

use std::{
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use anyhow::Context;
use tokio::{
    sync::{broadcast, mpsc},
    task::JoinSet,
};

use crate::{
    measurement::MeasurementBuffer,
    pipeline::{Transform, TransformError},
};

/// Controls the transforms of a measurement pipeline.
///
/// There can be a maximum of 64 transforms for the moment.
pub struct TransformControl {
    tasks: JoinSet<anyhow::Result<()>>,
    active_bitset: Arc<AtomicU64>,
    names_by_bitset_position: Vec<TransformName>,
}

impl TransformControl {
    pub fn handle_message(&mut self, msg: ControlMessage) {
        let mut bitset = self.active_bitset.load(Ordering::Relaxed);
        for (i, name) in self.names_by_bitset_position.iter().enumerate() {
            if name.matches(&msg.selector) {
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
}

#[derive(PartialEq, Eq)]
pub struct TransformName {
    plugin: String,
    transform: String,
}

impl TransformName {
    pub fn matches(&self, selector: &TransformSelector) -> bool {
        match selector {
            TransformSelector::Single(full_name) => self == full_name,
            TransformSelector::Plugin(plugin_name) => &self.plugin == plugin_name,
            TransformSelector::All => true,
        }
    }
}

impl fmt::Display for TransformName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.plugin, self.transform)
    }
}

pub enum TransformSelector {
    Single(TransformName),
    Plugin(String),
    All,
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
) -> anyhow::Result<()> {
    loop {
        if let Some(mut measurements) = rx.recv().await {
            // Update the list of active transforms.
            let current_flags = active_flags.load(Ordering::Relaxed);

            // Run the enabled transforms. If one of them fails, the ability to continue running depends on the error type.
            for (i, t) in &mut transforms.iter_mut().enumerate() {
                let t_flag = 1 << i;
                if current_flags & t_flag != 0 {
                    let (name, transform) = t;
                    match transform.apply(&mut measurements) {
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
