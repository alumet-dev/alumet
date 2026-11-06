//! The perf [`Source`]: how the values of the events are reported to Alumet.
use std::collections::HashMap;

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};

use crate::counters::{Counters, GroupedEvent, Observed};
use crate::event::{ParsedEvent, Scope};
use crate::resource;

/// The source of a process, a cgroup or the whole machine.
pub(super) struct PerfEventSource {
    counters: Counters,
    /// The events counted, in the order given to the counters, and their metric.
    events: Vec<(ParsedEvent, TypedMetricId<u64>)>,
    consumer: ResourceConsumer,
    /// The resource of each `(event, cpu)` read, computed once (it may read sysfs).
    resources: HashMap<(usize, Option<u32>), Resource>,
}

impl PerfEventSource {
    /// Opens the counters of `events` (each paired with its metric) on `observed`.
    pub(super) fn new(
        observed: &Observed,
        events: &[(GroupedEvent, TypedMetricId<u64>)],
        auto_scale: bool,
        consumer: ResourceConsumer,
    ) -> anyhow::Result<Self> {
        let grouped: Vec<GroupedEvent> = events.iter().map(|(e, _)| e.clone()).collect();
        Ok(Self {
            counters: Counters::open(observed, &grouped, auto_scale)?,
            events: events.iter().map(|(e, m)| (e.event.clone(), *m)).collect(),
            consumer,
            resources: HashMap::new(),
        })
    }
}

impl Source for PerfEventSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let readings = self.counters.read()?;
        for r in readings {
            let (event, metric) = &self.events[r.event];
            let resource = self
                .resources
                .entry((r.event, r.cpu))
                .or_insert_with(|| resource_of(event, r.cpu))
                .clone();
            let mut point = MeasurementPoint::new(timestamp, *metric, resource, self.consumer.clone(), r.value)
                .with_attr("accuracy", r.accuracy.as_str());
            if let Scope::TaskAttached { binding: Some(b) } = event.scope() {
                point = point.with_attr("pmu", b.pmu.clone());
            }
            measurements.push(point);
        }
        Ok(())
    }
}

/// The resource measured by `event` on `cpu` (`None`: counted on a task, on any cpu).
fn resource_of(event: &ParsedEvent, cpu: Option<u32>) -> Resource {
    match (event.scope(), cpu) {
        (Scope::TaskAttached { binding }, None) => resource::for_process(binding.as_ref()),
        (Scope::TaskAttached { .. }, Some(cpu)) => resource::for_cgroup_on_cpu(cpu),
        (Scope::SystemWide { pmu, .. }, cpu) => {
            resource::for_system_wide_pmu(pmu, cpu.expect("system-wide events are counted per cpu"))
        }
    }
}
