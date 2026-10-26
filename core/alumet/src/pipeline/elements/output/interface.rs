use std::{
    collections::BTreeSet,
    future::Future,
    pin::Pin,
    time::Duration,
};

use crate::{
    measurement::MeasurementBuffer,
    metrics::{def::RawMetricId, registry::MetricRegistry},
    pipeline::elements::source::SourceStates,
};

use super::error::WriteError;

/// A blocking output that exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    /// Writes the measurements to the output.
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError>;
}

/// An asynchronous stream of measurements, to be used by an asynchronous output.
pub struct AsyncOutputStream(
    pub Pin<Box<dyn futures::Stream<Item = Result<MeasurementBuffer, StreamRecvError>> + Send>>,
); // TODO make opaque?

pub use crate::pipeline::util::channel::StreamRecvError;
pub type BoxedAsyncOutput = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>>;

/// Shared data that can be accessed by outputs.
pub struct OutputContext<'a> {
    pub metrics: &'a MetricRegistry,
    /// Current configuration of the managed sources (poll intervals),
    /// updated when the pipeline is reconfigured (e.g. via socket-control).
    pub sources: &'a SourceStates,
}

impl OutputContext<'_> {
    /// The current polling intervals of the managed sources that produce `metric`, deduplicated
    /// and sorted. Returns an empty list if the metric's origin plugin is unknown, if that plugin
    /// has no managed source polled at a regular interval, or if the interval was changed to a
    /// non-interval trigger.
    ///
    /// Note: attribution is at plugin granularity, so if a plugin has several sources with
    /// different poll intervals, all of them are returned.
    pub fn poll_intervals_of(&self, metric: &RawMetricId) -> Vec<Duration> {
        match self.metrics.plugins_of(metric) {
            Some(plugins) => {
                let mut intervals = BTreeSet::new();
                for plugin in plugins {
                    intervals.extend(self.sources.poll_intervals(plugin));
                }
                intervals.into_iter().collect()
            }
            None => Vec::new(),
        }
    }
}
