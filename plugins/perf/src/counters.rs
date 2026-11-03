use std::{fs::File, io, sync::Arc};

use anyhow::Context;

use crate::event::{ParsedEvent, Scope};
use crate::group::{Accuracy, EventGroup, EventGroupBuilder, Target};
use crate::sysfs;

/// What to count on.
#[derive(Debug, Clone)]
pub enum Observed {
    /// A task (a thread: perf counts the given tid only); with `inherit`, also every task it
    /// creates after being opened.
    Task { tid: u32, inherit: bool },
    /// A cgroup (an open directory of the cgroup v2 hierarchy), on each cpu it may run on.
    Cgroup { fd: Arc<File> },
    /// The whole machine, for the system-wide PMUs (uncore, `power`, `cstate_*`, …).
    Machine,
}

/// An event to count, and the perf group it belongs to (`None`: a group of its own).
#[derive(Debug, Clone)]
pub struct GroupedEvent {
    pub event: ParsedEvent,
    pub group: Option<String>,
}

/// The value of an event.
#[derive(Debug, Clone, Copy)]
pub struct Reading {
    /// The index of the event in the list given to [`Counters::open`].
    pub event: usize,
    /// The cpu counted, for a cgroup or the machine (one counter per cpu); `None` for a task.
    pub cpu: Option<u32>,
    pub value: u64,
    pub accuracy: Accuracy,
}

/// The events of one perf group.
#[derive(Debug)]
struct GroupMembers {
    /// The index of each counter's event, in `add` order.
    events: Vec<usize>,
    cpu: Option<u32>,
    /// The PMU of the group's events: a perf group cannot span two PMUs.
    pmu_key: u64,
    group: Option<String>,
}

/// The perf groups counting a list of events on one [`Observed`]: one per `(cpu, pmu, group name)`,
/// and one per event without a group name.
pub struct Counters {
    groups: Vec<(EventGroup, GroupMembers)>,
}

impl Counters {
    /// Spreads `events` over perf groups, and opens and enables them on `observed`.
    pub fn open(observed: &Observed, events: &[GroupedEvent], auto_scale: bool) -> anyhow::Result<Self> {
        let mut groups: Vec<(EventGroupBuilder, GroupMembers)> = Vec::new();
        for (i, GroupedEvent { event, group }) in events.iter().enumerate() {
            for (target, cpu) in targets(observed, event)? {
                add_to_matching_group(&mut groups, target, cpu, event, i, group, auto_scale)
                    .with_context(|| format!("could not configure event {}", event.name()))?;
            }
        }
        log::debug!(
            "Opening perf groups on {observed:?}: {:?}",
            groups.iter().map(|(_, m)| m).collect::<Vec<_>>()
        );
        let groups = groups
            .into_iter()
            .map(|(group, members)| Ok((group.build()?, members)))
            .collect::<io::Result<_>>()?;
        Ok(Self { groups })
    }

    /// Reads every group.
    pub fn read(&mut self) -> anyhow::Result<Vec<Reading>> {
        let mut readings = Vec::new();
        for (group, members) in &mut self.groups {
            group.read()?;
            for (&event, &value) in members.events.iter().zip(group.values()) {
                readings.push(Reading {
                    event,
                    cpu: members.cpu,
                    value,
                    accuracy: group.accuracy(),
                });
            }
        }
        Ok(readings)
    }
}

/// Where to open `event` on `observed`, and the cpu of each target if it is per cpu.
fn targets(observed: &Observed, event: &ParsedEvent) -> anyhow::Result<Vec<(Target, Option<u32>)>> {
    Ok(match (event.scope(), observed) {
        (Scope::TaskAttached { .. }, Observed::Task { tid, inherit }) => vec![(
            Target::Task {
                tid: *tid,
                inherit: *inherit,
            },
            None,
        )],
        // Cgroup events need a cpu: one per cpu of the event's core PMU, or per online cpu.
        (Scope::TaskAttached { binding }, Observed::Cgroup { fd }) => {
            let cpus = match binding {
                Some(b) => b.cpus.clone(),
                None => sysfs::online_cpus().context("could not detect online CPUs")?,
            };
            cpus.into_iter()
                .map(|cpu| (Target::Cgroup { fd: fd.clone(), cpu }, Some(cpu)))
                .collect()
        }
        (Scope::SystemWide { cpus, .. }, Observed::Machine) => {
            cpus.iter().map(|&cpu| (Target::Cpu { cpu }, Some(cpu))).collect()
        }
        (Scope::SystemWide { .. }, _) => anyhow::bail!(
            "system-wide event {} can only be counted on the whole machine",
            event.name()
        ),
        (Scope::TaskAttached { .. }, Observed::Machine) => anyhow::bail!(
            "event {} cannot be counted on the whole machine: only system-wide events can",
            event.name()
        ),
    })
}

/// Adds the event `index` to the group matching `(cpu, pmu, group)`, opening it on `target` if none
/// exists yet. An event without a group name always opens a new group.
fn add_to_matching_group(
    groups: &mut Vec<(EventGroupBuilder, GroupMembers)>,
    target: Target,
    cpu: Option<u32>,
    event: &ParsedEvent,
    index: usize,
    group: &Option<String>,
    auto_scale: bool,
) -> anyhow::Result<()> {
    let pmu_key = event.pmu_group_key();
    let existing = groups
        .iter()
        .position(|(_, m)| group.is_some() && m.group == *group && m.cpu == cpu && m.pmu_key == pmu_key);
    let i = match existing {
        Some(i) => i,
        None => {
            let members = GroupMembers {
                events: Vec::new(),
                cpu,
                pmu_key,
                group: group.clone(),
            };
            groups.push((EventGroupBuilder::open(target, auto_scale)?, members));
            groups.len() - 1
        }
    };
    let (builder, members) = &mut groups[i];
    builder.add(event)?;
    members.events.push(index);
    Ok(())
}
