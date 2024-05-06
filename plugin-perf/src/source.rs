//! Source of measurements based on Linux perf_events.
use std::{fs::File, io};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{PollError, Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use itertools::Itertools;

use crate::cpu;

#[derive(Debug)]
pub enum Observable {
    /// Observe a process.
    ///
    /// `perf_event_open` can be called with `pid` and `cpu = -1` (any cpu)
    Process { pid: i32 },
    /// Observe a cgroup.
    ///
    /// Unlike processes, cgroups cannot be monitored with `cpu = -1`, a specific cpu id is required
    /// for `perf_event_open` (see https://github.com/torvalds/linux/blob/2c8159388952f530bd260e097293ccc0209240be/kernel/events/core.c#L12487)
    Cgroup { path: String, fd: File },
}

pub struct PerfEventSource {
    event_groups: Vec<EventGroup>,
}

struct EventGroup {
    perf_group: perf_event::Group,
    observed_resource: Resource,
    observed_consumer: ResourceConsumer,
    cpu_id: Option<u32>,
    counters: Vec<(perf_event::Counter, TypedMetricId<u64>)>,
}

impl Source for PerfEventSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for group in &mut self.event_groups {
            // read all counters in the group
            let counts = group.perf_group.read()?;

            // get some metadata about the measurement perimeter
            let resource = &group.observed_resource;
            let consumer = &group.observed_consumer;

            // TODO: check time_enabled and time_running to detect issues
            log::trace!(
                "Got perf_events measurements: time_enabled={:?}, time_running={:?}",
                counts.time_enabled(),
                counts.time_running()
            );

            // for each counter, push its value
            for (perf_counter, alumet_metric) in &group.counters {
                let value = counts[perf_counter];
                measurements.push(MeasurementPoint::new(
                    timestamp,
                    *alumet_metric,
                    resource.clone(),
                    consumer.clone(),
                    value,
                ))
            }
        }
        Ok(())
    }
}

/// Builder for the perf [`Source`].
pub struct PerfEventSourceBuilder {
    /// Something to observe.
    observable: Observable,
    /// One or multiple groups, all containing the same events.
    groups: Vec<EventGroup>,
    /// The available CPUs to monitor.
    online_cpus: Vec<u32>,
}

impl PerfEventSourceBuilder {
    pub fn observe(observable: Observable) -> anyhow::Result<Self> {
        Ok(Self {
            observable,
            groups: Vec::new(),
            online_cpus: cpu::online_cpus().context("could not detect online CPUs")?,
        })
    }

    pub fn add<E: perf_event::events::Event + Clone>(
        &mut self,
        event: E,
        alumet_metric: TypedMetricId<u64>,
    ) -> anyhow::Result<&mut Self> {
        // Returns a new [`perf_event::Builder`] configured to build a group of perf events.
        fn new_group_builder<'a>() -> perf_event::Builder<'a> {
            use perf_event::ReadFormat;

            // use the DUMMY event for the group leader, because its value is not included in the result of Group::read
            let mut builder = perf_event::Builder::new(perf_event::events::Software::DUMMY);
            builder.read_format(
                ReadFormat::GROUP | ReadFormat::TOTAL_TIME_ENABLED | ReadFormat::TOTAL_TIME_RUNNING | ReadFormat::ID,
            );
            builder
        }

        if self.groups.is_empty() {
            // create the group(s)
            match &self.observable {
                Observable::Process { pid } => {
                    // Observe the process on any cpu.

                    // build group
                    let mut perf_group = new_group_builder()
                        .observe_pid(*pid)
                        .any_cpu()
                        .build_group()
                        .with_context(|| format!("build_group with observe_pid({pid}).any_cpu()"))?;

                    // add event (the params must be the same)
                    let counter = perf_group
                        .add(&perf_event::Builder::new(event).observe_pid(*pid).any_cpu())
                        .with_context(|| format!("perf_group.add with observe_pid({pid}).any_cpu()"))?;

                    // add metadata
                    let group_with_info = EventGroup {
                        perf_group,
                        observed_resource: Resource::LocalMachine,
                        observed_consumer: ResourceConsumer::Process {
                            pid: u32::try_from(*pid).unwrap(),
                        },
                        cpu_id: None,
                        counters: vec![(counter, alumet_metric)],
                    };

                    // done
                    self.groups = vec![group_with_info];
                }
                Observable::Cgroup { path, fd } => {
                    // Observe the cgroup on each cpu separately (this is a restriction of perf_event_open).

                    // build one group per cpu
                    let mut groups = Vec::new();
                    for cpu_id in &self.online_cpus {
                        let cpu_id = *cpu_id as usize;

                        // build group
                        let mut perf_group = new_group_builder()
                            .observe_cgroup(fd)
                            .one_cpu(cpu_id)
                            .build_group()
                            .with_context(|| format!("build_group with observe_cgroup({path}).one_cpu({cpu_id})"))?;

                        // add event (the params must be the same)
                        let counter = perf_group
                            .add(
                                &perf_event::Builder::new(event.clone())
                                    .observe_cgroup(fd)
                                    .one_cpu(cpu_id),
                            )
                            .with_context(|| format!("perf_group.add with observe_cgroup({path}).one_cpu({cpu_id})"))?;

                        let group_with_info = EventGroup {
                            perf_group,
                            observed_resource: Resource::LocalMachine,
                            observed_consumer: ResourceConsumer::ControlGroup {
                                path: path.to_owned().into(),
                            },
                            cpu_id: None,
                            counters: vec![(counter, alumet_metric)],
                        };
                        groups.push(group_with_info);
                    }
                    self.groups = groups;
                }
            }
        } else {
            // add to the group(s)
            for group in &mut self.groups {
                let mut event_builder = perf_event::Builder::new(event.clone());

                // Compute the event params to be the same as the group's params.
                match &self.observable {
                    Observable::Process { pid } => {
                        event_builder.observe_pid(*pid).any_cpu();
                    }
                    Observable::Cgroup { path: _, fd } => {
                        event_builder.observe_cgroup(fd).one_cpu(group.cpu_id.unwrap() as usize);
                    }
                }

                let counter = group.perf_group.add(&event_builder).with_context(|| {
                    format!(
                        "existing perf_group.add(event_builder), group resource={:?}, consumer={:?}, cpu={:?}",
                        group.observed_resource, group.observed_consumer, group.cpu_id
                    )
                })?;
                group.counters.push((counter, alumet_metric))
            }
        }
        Ok(self)
    }

    pub fn build(mut self) -> io::Result<PerfEventSource> {
        log::debug!(
            "Built PerfEventSource with groups [{}]",
            self.groups
                .iter()
                .map(|g| format!(
                    "{{resource: {:?}, consumer: {:?}, cpu: {:?}, events: {:?}}}",
                    g.observed_resource, g.observed_consumer, g.cpu_id, g.counters
                ))
                .join(", ")
        );
        for group in &mut self.groups {
            group.perf_group.enable()?;
        }

        Ok(PerfEventSource {
            event_groups: self.groups,
        })
    }
}
