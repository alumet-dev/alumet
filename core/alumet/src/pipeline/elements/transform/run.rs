//! Runtime implementation of the task that executes transforms.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use tokio::sync::{broadcast, mpsc};

use crate::{
    measurement::MeasurementBuffer,
    metrics::online::MetricReader,
    pipeline::{error::PipelineError, naming::TransformName},
};

use super::{Transform, TransformContext, error::TransformError};

pub async fn run_all_in_order(
    mut transforms: Vec<(TransformName, Box<dyn Transform>)>,
    mut rx: mpsc::Receiver<MeasurementBuffer>,
    tx: broadcast::Sender<MeasurementBuffer>,
    active_flags: Arc<AtomicU64>,
    metrics_reader: MetricReader,
) -> Result<(), PipelineError> {
    log::trace!(
        "Running transforms: {}",
        transforms
            .iter()
            .map(|(name, _)| name.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    loop {
        if let Some(mut measurements) = rx.recv().await {
            // Update the list of active transforms.
            let current_flags = active_flags.load(Ordering::Relaxed);
            log::trace!("current 'enabled' bitset: {current_flags}");

            // Build the transform context.
            // This will block the publication of any modification to the MetricRegistry until the context is dropped.
            // TODO this need to change: if transforms take a "long" time to execute, the registry will be blocked for a long time,
            // which is bad. Usually, transforms don't need to use the MetricRegistry for a long time (see next TODO).
            // Or, we could store a separate copy of the registry just for transforms.
            // TODO: this point should be emphasized in the transforms docs so that people don't implement bad transforms.
            let metrics = &metrics_reader.read().await;
            let ctx = TransformContext { metrics };

            // Run the enabled transforms. If one of them fails, the ability to continue running depends on the error type.
            for (i, (name, t)) in &mut transforms.iter_mut().enumerate() {
                let t_flag = 1 << i;
                if current_flags & t_flag != 0 {
                    match t.apply(&mut measurements, &ctx) {
                        Ok(()) => (),
                        Err(TransformError::UnexpectedInput(e)) => {
                            log::error!("Transform {name} received unexpected measurements: {e:#}");
                        }
                        Err(TransformError::Fatal(e)) => {
                            log::error!("Fatal error in transform {name} (this breaks the transform task!): {e:?}");
                            return Err(PipelineError::for_element(name.to_owned(), e));
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
