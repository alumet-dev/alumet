use tokio::sync::mpsc;

use crate::pipeline::Source;

use super::SourceCheck;

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
    fn poll(
        &mut self,
        measurements: &mut crate::measurement::MeasurementAccumulator,
        timestamp: crate::measurement::Timestamp,
    ) -> Result<(), crate::pipeline::elements::error::PollError> {
        let check = self.in_rx.try_recv().unwrap().0;
        (check.make_input)();
        let res = self.source.poll(measurements, timestamp);
        (check.check_output)(measurements.as_inner());
        self.out_tx.try_send(SourceDone).unwrap();
        res
    }
}
