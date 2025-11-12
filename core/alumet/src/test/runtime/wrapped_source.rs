use std::{
    panic::{self, AssertUnwindSafe},
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use tokio::sync::mpsc;

use crate::{
    measurement::{MeasurementAccumulator, Timestamp},
    metrics::online::MetricReader,
    pipeline::{Source, elements::error::PollError},
    test::runtime::SourceCheckOutputContext,
};

use super::{SourceCheck, pretty::PrettyAny};

/// Wraps a source and applies checks to it on trigger.
pub struct WrappedManagedSource {
    /// The source to test.
    pub source: Box<dyn Source>,
    pub in_rx: mpsc::Receiver<SetSourceCheck>,
    pub out_tx: mpsc::Sender<SourceDone>,

    /// Metrics reader that will be provided later.
    pub metrics_r: Arc<Mutex<Option<MetricReader>>>,
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

        // TODO allow make_input to change the timestamp?

        // poll the source, catch any panic
        log::trace!("polling underlying source");
        self.source.poll(measurements, timestamp)?;

        // get read access to the MetricRegistry
        log::trace!("get access to the metric registry");
        let metrics_lock = self.metrics_r.lock().unwrap();
        let metrics_r = metrics_lock
            .as_ref()
            .expect("MetricReader should be set before the pipeline starts");
        tokio::task::block_in_place(move || {
            let mut check_ctx = SourceCheckOutputContext {
                measurements: measurements.as_inner(),
                metrics: &metrics_r.blocking_read(),
            };

            // check output
            log::trace!("applying check");
            (check.check_output)(&mut check_ctx);
        });

        self.out_tx.try_send(SourceDone).unwrap();
        log::trace!("wrapped source done");
        Ok(())
    }
}
