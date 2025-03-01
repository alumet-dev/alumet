use std::panic::{self, AssertUnwindSafe};

use anyhow::anyhow;
use tokio::sync::mpsc;

use crate::{
    measurement::{MeasurementAccumulator, Timestamp},
    pipeline::{elements::error::PollError, Source},
};

use super::{pretty::PrettyAny, SourceCheck};

/// Wraps a source and applies checks to it on trigger.
pub struct WrappedManagedSource {
    /// The source to test.
    pub source: Box<dyn Source>,
    pub in_rx: mpsc::Receiver<SetSourceCheck>,
    pub out_tx: mpsc::Sender<SourceDone>,
}

pub struct SetSourceCheck(pub SourceCheck);
pub struct SourceDone;

impl Source for WrappedManagedSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let res = panic::catch_unwind(AssertUnwindSafe(|| self.test_poll(measurements, timestamp)));
        match res {
            Ok(Ok(ok)) => Ok(ok),
            Ok(Err(e)) => Err(e),
            Err(panic) => Err(PollError::Fatal(anyhow!("source panicked: {:?}", PrettyAny(panic)))),
        }
    }
}

impl WrappedManagedSource {
    fn test_poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        // prepare input
        log::trace!("receiving next check...");
        let check = self.in_rx.try_recv().unwrap().0;
        (check.make_input)();

        // poll the source, catch any panic
        log::trace!("polling underlying source");
        self.source.poll(measurements, timestamp)?;

        // check output
        log::trace!("applying check");
        (check.check_output)(measurements.as_inner());
        self.out_tx.try_send(SourceDone).unwrap();

        log::trace!("wrapped source done");
        Ok(())
    }
}
