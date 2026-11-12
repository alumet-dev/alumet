use std::{collections::BTreeSet, future::Future, pin::Pin, time::Duration};

use crate::{
    measurement::{MeasurementBuffer, MeasurementPoint, POLL_INTERVAL_ATTRIBUTE},
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
    /// The current polling intervals that apply to `measurements`, deduplicated
    /// and sorted. Returns an empty list if no interval is known (e.g. manual triggers).
    ///
    /// The intervals come from the [`SourceTag`](crate::measurement::SourceTag) of the buffer, so they are exact
    /// per source (several sources may produce the same metric at different intervals)
    /// and survive serialization (e.g. relay). When the tag is missing or incomplete,
    /// the method falls back to the live config of the source, then to the
    /// plugin granularity: all intervals of all managed sources of the plugins
    /// that registered `metric`.
    pub fn poll_intervals_of(&self, measurements: &MeasurementBuffer, metric: &RawMetricId) -> Vec<Duration> {
        if let Some(tag) = &measurements.producer {
            // exact, self-describing case: the interval was snapshotted at flush time
            if let Some(interval) = tag.poll_interval {
                return vec![interval];
            }
            // local source: read its live config
            if let Some(interval) = self.sources.poll_interval_of(&tag.name) {
                return vec![interval];
            }
        }
        // fallback: all intervals of the plugins that registered this metric
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

    /// The poll interval (in milliseconds) to report for a single measurement point,
    /// as a formatted string, or `None` if it cannot be determined.
    ///
    /// This is the helper meant to be used by outputs that expose attributes/labels:
    /// - if the point carries a [`POLL_INTERVAL_ATTRIBUTE`] (stamped by the relay client
    ///   on relayed points), that value is used, since the producing source lives on
    ///   another agent and is unknown to this pipeline;
    /// - otherwise, the point is local: the interval is derived from the
    ///   [`SourceTag`](crate::measurement::SourceTag) of the buffer (exact per source)
    ///   and the live source config, so it follows dynamic reconfigurations
    ///   (e.g. socket-control `set-period`).
    pub fn poll_interval_for(&self, measurements: &MeasurementBuffer, point: &MeasurementPoint) -> Option<String> {
        // points relayed from another agent carry their interval as an attribute
        if let Some((_, interval)) = point.attributes().find(|(k, _)| *k == POLL_INTERVAL_ATTRIBUTE) {
            return Some(interval.to_string());
        }
        // local measurements: derive from the buffer's source
        let intervals = self.poll_intervals_of(measurements, &point.metric);
        if intervals.is_empty() {
            None
        } else {
            Some(
                intervals
                    .iter()
                    .map(|d| d.as_millis().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU8;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::measurement::{MeasurementPoint, SourceTag, Timestamp, WrappedMeasurementValue};
    use crate::metrics::def::Metric;
    use crate::metrics::duplicate::{DuplicateCriteria, DuplicateReaction};
    use crate::pipeline::elements::source::task_controller::{SharedSourceConfig, TriggerConfig};
    use crate::pipeline::naming::{PluginName, SourceName};
    use crate::resources::{Resource, ResourceConsumer};
    use crate::units::Unit;

    use super::*;

    fn source_config(interval: Option<Duration>) -> Arc<SharedSourceConfig> {
        Arc::new(SharedSourceConfig {
            change_notifier: tokio::sync::Notify::new(),
            atomic_state: AtomicU8::new(0),
            trigger: Mutex::new(TriggerConfig {
                new_trigger: None,
                manual_trigger: None,
            }),
            poll_interval: Mutex::new(interval),
        })
    }

    #[test]
    fn poll_interval_for_local_and_relayed_points() {
        // one metric, registered by plugin "local", produced by a source polled at 10s
        let mut registry = MetricRegistry::new();
        let metric = registry
            .register(
                Metric {
                    name: "m".to_owned(),
                    description: String::new(),
                    value_type: crate::measurement::WrappedMeasurementType::U64,
                    unit: Unit::Unity.into(),
                },
                DuplicateCriteria::Strict,
                DuplicateReaction::Error,
            )
            .unwrap();
        registry.attribute_to_plugin(metric, &PluginName("local".to_owned()));

        let sources = SourceStates::default();
        sources.add_source(
            &SourceName::from_str("local", "kernel"),
            source_config(Some(Duration::from_secs(10))),
        );

        let ctx = OutputContext {
            metrics: &registry,
            sources: &sources,
        };

        let point = || {
            MeasurementPoint::new_untyped(
                Timestamp::now(),
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(1),
            )
        };

        // 1. local point, buffer tagged with an interval snapshot -> exact value from the tag
        let mut tagged = MeasurementBuffer::new();
        tagged.producer = Some(SourceTag {
            name: SourceName::from_str("local", "kernel"),
            poll_interval: Some(Duration::from_millis(250)),
        });
        assert_eq!(ctx.poll_interval_for(&tagged, &point()).as_deref(), Some("250"));

        // 2. untagged buffer -> live config of the registered plugin's sources
        let untagged = MeasurementBuffer::new();
        assert_eq!(ctx.poll_interval_for(&untagged, &point()).as_deref(), Some("10000"));

        // 3. relayed point carrying the interval attribute -> the attribute wins
        let mut relayed = point();
        relayed.add_attr(POLL_INTERVAL_ATTRIBUTE, "777");
        assert_eq!(ctx.poll_interval_for(&untagged, &relayed).as_deref(), Some("777"));
    }
}
