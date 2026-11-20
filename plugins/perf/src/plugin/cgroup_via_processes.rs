//! The plugin side of [`crate::cgroup_via_processes`] (`cgroup_counting = { via_processes = {} }`):
//! a source that counts a cgroup through its processes, and scans them in the background.
//! Self-contained on purpose: delete this file with that module once the kernels have the fix.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;

use alumet::measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp};
use alumet::metrics::TypedMetricId;
use alumet::pipeline::{Source, elements::error::PollError};
use alumet::resources::{Resource, ResourceConsumer};

use crate::cgroup_via_processes::CgroupProcesses;
use crate::counters::GroupedEvent;
use crate::event::{ParsedEvent, Scope};
use crate::resource;

/// The source of a cgroup counted through its processes, on the kernels that miscount their
/// multiplexed events (see [`crate::cgroup_via_processes`]).
pub(super) struct CgroupProcessesSource {
    processes: Arc<Mutex<CgroupProcesses>>,
    /// The events counted, in the order given to the processes, and their metric.
    events: Vec<(ParsedEvent, TypedMetricId<u64>)>,
    consumer: ResourceConsumer,
    /// The resource of each `(event, cpu)` read, computed once (it may read sysfs).
    resources: HashMap<(usize, Option<u32>), Resource>,
}

impl CgroupProcessesSource {
    /// `events`, each paired with its metric, in the order given to [`CgroupProcesses`]. The cgroup
    /// `dir` is scanned once now, then every `scan_interval`, in a thread that stops with the source.
    pub(super) fn new(
        dir: PathBuf,
        events: &[(GroupedEvent, TypedMetricId<u64>)],
        auto_scale: bool,
        consumer: ResourceConsumer,
        scan_interval: Duration,
    ) -> anyhow::Result<Self> {
        log::info!("Starting to observe cgroup {} through its processes...", dir.display());
        let processes = Arc::new(Mutex::new(CgroupProcesses::new(
            dir,
            events.iter().map(|(e, _)| e.clone()).collect(),
            auto_scale,
        )));
        processes.lock().unwrap().scan();

        // An uncounted process loses what it does before it is found: read cgroup.procs much more
        // often than the poll, in a thread that stops once the source is dropped.
        let weak = Arc::downgrade(&processes);
        std::thread::Builder::new()
            .name("perf-cgroup-scan".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(scan_interval);
                    let Some(processes) = weak.upgrade() else { break };
                    processes.lock().unwrap().scan();
                }
            })
            .context("spawning the cgroup scanner")?;

        Ok(Self {
            processes,
            events: events.iter().map(|(g, m)| (g.event.clone(), *m)).collect(),
            consumer,
            resources: HashMap::new(),
        })
    }
}

impl Source for CgroupProcessesSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let readings = self.processes.lock().unwrap().read();
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

/// The resource measured by `event` on `cpu` (`None`: counted through a task, on any cpu).
fn resource_of(event: &ParsedEvent, cpu: Option<u32>) -> Resource {
    match (event.scope(), cpu) {
        (Scope::TaskAttached { binding }, None) => resource::for_process(binding.as_ref()),
        (Scope::TaskAttached { .. }, Some(cpu)) => resource::for_cgroup_on_cpu(cpu),
        (Scope::SystemWide { pmu, .. }, cpu) => {
            resource::for_system_wide_pmu(pmu, cpu.expect("system-wide events are counted per cpu"))
        }
    }
}
