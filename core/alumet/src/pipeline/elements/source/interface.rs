use std::{future::Future, pin::Pin};

use crate::measurement::{MeasurementAccumulator, Timestamp};

use super::error::PollError;

pub type AutonomousSource = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    /// Polls the source for new measurements.
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError>;

    /// Resets the source’s internal state when it is paused.
    /// A default no-op implementation is provided, but you may want to override it to properly clear the source’s state.
    /// This helps avoid measurement inconsistencies when the source resumes after a pause (e.g: for CounterDiff or "last timestamp" values).
    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
