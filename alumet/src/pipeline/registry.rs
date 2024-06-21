//! Registry of metrics common to the whole pipeline.

use left_right::{Absorb, ReadGuard, ReadHandle, WriteHandle};
use tokio::sync::oneshot;

use crate::metrics::{Metric, MetricCreationError, MetricRegistry, RawMetricId};

enum RegistryOp {
    Register(
        Vec<Metric>,
        OnDuplicateMetric,
        Option<oneshot::Sender<Result<Vec<RawMetricId>, MetricCreationError>>>,
    ),
}

pub enum OnDuplicateMetric {
    Error,
    Rename { suffix: String },
}

impl Absorb<RegistryOp> for MetricRegistry {
    fn absorb_first(&mut self, operation: &mut RegistryOp, _other: &Self) {
        match operation {
            RegistryOp::Register(metrics, on_duplicate, result_tx) => {
                let res = match on_duplicate {
                    OnDuplicateMetric::Error => self.extend(metrics.clone()),
                    OnDuplicateMetric::Rename { suffix } => Ok(self.extend_infallible(metrics.clone(), suffix)),
                };
                // Leave the option empty because we only want to send the message once.
                if let Some(tx) = result_tx.take() {
                    tx.send(res).unwrap();
                }
            }
        }
    }

    fn sync_with(&mut self, first: &Self) {
        self.metrics_by_id = first.metrics_by_id.clone();
        self.metrics_by_name = first.metrics_by_name.clone();
    }
}

#[derive(Clone)]
pub struct SharedRegistryReader(ReadHandle<MetricRegistry>);

pub struct SharedRegistryWriter(WriteHandle<MetricRegistry, RegistryOp>);

impl SharedRegistryReader {
    pub fn read(&self) -> ReadGuard<MetricRegistry> {
        self.0
            .enter()
            .expect("WriteHandle<MetricRegistry> should not be dropped before the pipeline tasks")
    }
}

impl SharedRegistryWriter {
    pub async fn register_multiple(
        &mut self,
        metrics: Vec<Metric>,
        on_duplicate: OnDuplicateMetric,
    ) -> Result<Vec<RawMetricId>, MetricCreationError> {
        // Use a oneshot channel to asynchronously get the result of the operation.
        let (tx, rx) = oneshot::channel();
        
        // Add the changes to the left_right internal queue.
        self.0.append(RegistryOp::Register(metrics, on_duplicate, Some(tx)));

        // Apply the changes and swap the left_right to make them visible to readers.
        self.0.publish();
        
        // Get the result of the metric registration.
        rx.await.unwrap()
    }
}
