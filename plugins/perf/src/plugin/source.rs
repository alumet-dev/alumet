//! The perf [`Source`]: how the configured events are spread over [`EventGroup`]s, and how their
//! values are reported to Alumet.
use std::{fs::File, io, sync::Arc};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use itertools::Itertools;

use crate::event::{ParsedEvent, Scope};
use crate::group::{EventGroup, EventGroupBuilder, Target};
use crate::resource;
use crate::sysfs;

#[derive(Debug)]
pub enum Observable {
    /// Observe a process.
    Process { pid: i32 },
    /// Observe a cgroup.
    Cgroup { path: String, fd: Arc<File> },
    /// Observe the whole machine, for the system-wide PMUs (uncore, `power`, `cstate_*`, …).
    Machine,
}

pub struct PerfEventSource {
    groups: Vec<(EventGroup, GroupInfo)>,
}

/// What a group measures, and the Alumet metric of each of its counters (in `add` order).
struct GroupInfo {
    metrics: Vec<TypedMetricId<u64>>,
    /// Partition key: there is one group per `(cpu_id, key)` (a perf group cannot span two PMUs).
    key: u64,
    cpu_id: Option<u32>,
    resource: Resource,
    consumer: ResourceConsumer,
    /// The core PMU name, reported as the `pmu` attribute; `None` for events with no specific PMU.
    pmu_attr: Option<String>,
}

impl Source for PerfEventSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for (group, info) in &mut self.groups {
            group.read()?;
            let accuracy = group.accuracy().as_str();
            for (metric, value) in info.metrics.iter().zip(group.values()) {
                let mut point =
                    MeasurementPoint::new(timestamp, *metric, info.resource.clone(), info.consumer.clone(), *value)
                        .with_attr("accuracy", accuracy);
                if let Some(pmu) = &info.pmu_attr {
                    point = point.with_attr("pmu", pmu.clone());
                }
                measurements.push(point)
            }
        }
        Ok(())
    }
}

/// Builder for the perf [`Source`] of a process, a cgroup or the whole machine.
pub struct PerfEventSourceBuilder {
    /// Something to observe.
    observable: Observable,
    /// The groups opened so far, one per `(cpu, pmu)`.
    groups: Vec<(EventGroupBuilder, GroupInfo)>,
    /// The available CPUs to monitor.
    online_cpus: Vec<u32>,
    /// Activate auto_scaling in case of detected multiplexing
    multiplexing_auto_scale: bool,
}

impl PerfEventSourceBuilder {
    pub fn observe(observable: Observable, multiplexing_auto_scale: bool) -> anyhow::Result<Self> {
        Ok(Self {
            observable,
            groups: Vec::new(),
            online_cpus: sysfs::online_cpus().context("could not detect online CPUs")?,
            multiplexing_auto_scale,
        })
    }

    pub fn add(&mut self, event: &ParsedEvent, alumet_metric: TypedMetricId<u64>) -> anyhow::Result<&mut Self> {
        let key = event.event.pmu_group_key();
        let auto_scale = self.multiplexing_auto_scale;

        match (&event.scope, &self.observable) {
            (Scope::TaskAttached { binding }, Observable::Process { pid }) => {
                let target = Target::Process { pid: *pid };
                let info = GroupInfo {
                    metrics: Vec::new(),
                    key,
                    cpu_id: None,
                    resource: resource::for_process(binding.as_ref()),
                    consumer: ResourceConsumer::Process {
                        pid: u32::try_from(*pid).unwrap(),
                    },
                    pmu_attr: binding.as_ref().map(|b| b.pmu.clone()),
                };
                self.add_to_group(target, info, event, alumet_metric)?;
            }
            (Scope::TaskAttached { binding }, Observable::Cgroup { path, fd }) => {
                // Owned copies, so that `self` is no longer borrowed while groups are added.
                let (path, fd) = (path.clone(), fd.clone());
                let cpus = binding
                    .as_ref()
                    .map_or_else(|| self.online_cpus.clone(), |b| b.cpus.clone());
                for cpu in cpus {
                    let target = Target::Cgroup { fd: fd.clone(), cpu };
                    let info = GroupInfo {
                        metrics: Vec::new(),
                        key,
                        cpu_id: Some(cpu),
                        resource: resource::for_cgroup_on_cpu(cpu),
                        consumer: ResourceConsumer::ControlGroup {
                            path: path.clone().into(),
                        },
                        pmu_attr: binding.as_ref().map(|b| b.pmu.clone()),
                    };
                    self.add_to_group(target, info, event, alumet_metric)?;
                }
            }
            (Scope::SystemWide { pmu, cpus }, Observable::Machine) => {
                // One group per event and per cpu.
                for &cpu in cpus {
                    let mut group = EventGroupBuilder::open(Target::Cpu { cpu }, auto_scale)?;
                    group.add(event)?;
                    let info = GroupInfo {
                        metrics: vec![alumet_metric],
                        key,
                        cpu_id: Some(cpu),
                        resource: resource::for_system_wide_pmu(pmu, cpu),
                        consumer: ResourceConsumer::LocalMachine,
                        pmu_attr: None,
                    };
                    self.groups.push((group, info));
                }
            }
            (Scope::SystemWide { .. }, _) => anyhow::bail!(
                "system-wide event {} can only be counted on the whole machine",
                event.name
            ),
            (Scope::TaskAttached { .. }, Observable::Machine) => anyhow::bail!(
                "event {} cannot be counted on the whole machine: only system-wide events can",
                event.name
            ),
        }
        Ok(self)
    }

    /// Adds `event` to the group matching `info`'s `(cpu_id, key)`, opening it on `target` if none
    /// exists yet.
    fn add_to_group(
        &mut self,
        target: Target,
        info: GroupInfo,
        event: &ParsedEvent,
        alumet_metric: TypedMetricId<u64>,
    ) -> anyhow::Result<()> {
        let existing = self
            .groups
            .iter()
            .position(|(_, g)| g.cpu_id == info.cpu_id && g.key == info.key);
        let i = match existing {
            Some(i) => i,
            None => {
                let group = EventGroupBuilder::open(target, self.multiplexing_auto_scale)?;
                self.groups.push((group, info));
                self.groups.len() - 1
            }
        };
        let (group, info) = &mut self.groups[i];
        group.add(event)?;
        info.metrics.push(alumet_metric);
        Ok(())
    }

    pub fn build(self) -> io::Result<PerfEventSource> {
        log::debug!(
            "Built PerfEventSource with groups [{}]",
            self.groups
                .iter()
                .map(|(_, info)| format!(
                    "{{resource: {:?}, consumer: {:?}, cpu: {:?}, metrics: {:?}}}",
                    info.resource, info.consumer, info.cpu_id, info.metrics
                ))
                .join(", ")
        );
        let groups = self
            .groups
            .into_iter()
            .map(|(group, info)| Ok((group.build()?, info)))
            .collect::<io::Result<_>>()?;
        Ok(PerfEventSource { groups })
    }
}
