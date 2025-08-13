use std::{future::Future, pin::Pin};

use crate::measurement::{MeasurementAccumulator, Timestamp};

use super::error::PollError;

pub type AutonomousSource = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// Produces measurements related to some metrics.
pub trait Source: Send {
    /// Polls the source for new measurements.
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError>;
}
