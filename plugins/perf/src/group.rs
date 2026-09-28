//! perf groups: counters that are scheduled onto the PMU together, and read together.
use std::{fs::File, io, sync::Arc};

use anyhow::Context;

use crate::event::{self, ParsedEvent, Scope};
use crate::multiplexing::{GroupCounters, Snapshot};

pub use crate::multiplexing::Accuracy;

/// What a group counts, and on which cpu. Every counter of a group shares it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Target {
    /// A process, on any cpu.
    Process { pid: u32 },
    /// A cgroup, on one cpu.
    ///
    /// Unlike processes, cgroups cannot be monitored with `cpu = -1`, a specific cpu id is required
    /// for `perf_event_open` (see <https://github.com/torvalds/linux/blob/2c8159388952f530bd260e097293ccc0209240be/kernel/events/core.c#L12487>)
    Cgroup { fd: Arc<File>, cpu: u32 },
    /// Everything running on one cpu. Required by the system-wide PMUs (uncore, `power`, `cstate_*`, …).
    Cpu { cpu: u32 },
}

/// Identifies a counter within its [`EventGroup`]: the index of the [`EventGroupBuilder::add`] that
/// created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CounterId(usize);

/// A perf group being filled: counters can be added, but it does not count yet.
pub struct EventGroupBuilder {
    target: Target,
    perf_group: perf_event::Group,
    counters: Vec<perf_event::Counter>,
    /// The PMU of the group's events, set by the first [`add`](Self::add): a perf group cannot span
    /// two PMUs.
    pmu_key: Option<u64>,
    /// Whether the events are pinned to a PMU covering only part of the CPUs (a hybrid pmu).
    partial_pmu: bool,
    /// Whether to extrapolate the values when the group is multiplexed.
    auto_scale: bool,
}

/// A counting perf group: its counters are scheduled onto the PMU together, and read together.
pub struct EventGroup {
    target: Target,
    perf_group: perf_event::Group,
    counters: Vec<perf_event::Counter>,
    partial_pmu: bool,
    auto_scale: bool,
    scaling: GroupCounters,
    /// Whether the previous poll found the group starved, so that we only warn on the transition
    /// into starvation and not on every poll while it lasts.
    starved: bool,
}

impl EventGroupBuilder {
    /// Open an empty group. Add the counters with [`add`](Self::add), then [`build`](Self::build) it.
    pub fn open(target: Target, auto_scale: bool) -> anyhow::Result<Self> {
        let perf_group = {
            // A `DUMMY` software leader: its value is excluded from `Group::read`, it just anchors the
            // group and carries the read format shared by every counter of the group.
            let mut leader = perf_event::Group::builder();
            attach_to(&mut leader, &target);
            if let Target::Cpu { .. } = target {
                include_all_domains(&mut leader);
            }
            leader
                .build_group()
                .with_context(|| format!("building perf group ({target:?})"))?
        };
        Ok(Self {
            target,
            perf_group,
            counters: Vec::new(),
            pmu_key: None,
            partial_pmu: false,
            auto_scale,
        })
    }

    pub fn add(&mut self, event: &ParsedEvent) -> anyhow::Result<CounterId> {
        let key = event.pmu_group_key();
        if self.pmu_key.is_some_and(|k| k != key) {
            anyhow::bail!(
                "event {} is not on the same PMU as the rest of the group (a perf group cannot span two PMUs)",
                event.name()
            );
        }

        let mut builder = perf_event::Builder::new(event.encoding());
        attach_to(&mut builder, &self.target);
        match (event.scope(), &self.target) {
            (Scope::TaskAttached { binding }, Target::Process { .. }) => {
                event.configure(&mut builder);
                self.partial_pmu = binding.as_ref().is_some_and(|b| b.partial);
            }
            // A cgroup is opened per-cpu, one cpu per group: the pmu-coverage reasoning does not
            // apply, so keep the standard accounting.
            (Scope::TaskAttached { .. }, _) => event.configure(&mut builder),
            // System-wide PMUs (RAPL/uncore/cstate) reject the `exclude_*` domain bits, and the
            // domain modifiers (`#u`/`#k`/`#h`) are meaningless for them anyway.
            (Scope::SystemWide { .. }, Target::Cpu { .. }) => include_all_domains(&mut builder),
            (Scope::SystemWide { .. }, _) => {
                anyhow::bail!("system-wide event {} can only be counted on a cpu target", event.name())
            }
        }
        let counter = self
            .perf_group
            .add(&builder)
            .with_context(|| format!("adding event {} to perf group ({:?})", event.name(), self.target))?;

        self.pmu_key = Some(key);
        self.counters.push(counter);
        Ok(CounterId(self.counters.len() - 1))
    }

    /// Start counting: no counter can be added afterwards.
    pub fn build(mut self) -> io::Result<EventGroup> {
        self.perf_group.enable()?;
        Ok(EventGroup {
            scaling: GroupCounters::new(self.counters.len()),
            target: self.target,
            perf_group: self.perf_group,
            counters: self.counters,
            partial_pmu: self.partial_pmu,
            auto_scale: self.auto_scale,
            starved: false,
        })
    }
}

impl EventGroup {
    /// Reads the group, updates the corrected cumulative value of each of its counters, and logs
    /// anything worth reporting about the multiplexing.
    pub fn read(&mut self) -> anyhow::Result<()> {
        use crate::multiplexing::Interval;

        let counts = self.perf_group.read().context("reading perf group")?;

        // Always available: `Group::builder` asks for TOTAL_TIME_ENABLED and TOTAL_TIME_RUNNING.
        let (Some(time_enabled), Some(time_running)) = (counts.time_enabled(), counts.time_running()) else {
            anyhow::bail!(
                "perf_events did not report time_enabled/time_running, which are required to detect multiplexing"
            );
        };
        let now = Snapshot {
            time_enabled: time_enabled.as_nanos(),
            time_running: time_running.as_nanos(),
            values: self.counters.iter().map(|counter| counts[counter]).collect(),
        };

        let auto_scale = self.auto_scale;
        let interval = self.scaling.account(now, auto_scale, self.partial_pmu);
        // Only warn on the transition: starvation can last for the whole lifetime of the source.
        let starved = interval == Interval::Starved;
        let just_starved = starved && !self.starved;
        self.starved = starved;
        match interval {
            Interval::Idle | Interval::Exact | Interval::Partial => (),
            Interval::Multiplexed { running, enabled } => {
                log::debug!(
                    "perf group ({:?}) was only on the PMU {:.1}% of the time, its values are {}",
                    self.target,
                    100.0 * (running as f64) / (enabled as f64),
                    if auto_scale { "extrapolated" } else { "underestimated" },
                );
            }
            Interval::Starved if just_starved => {
                log::warn!(
                    "perf group ({:?}) ran but never made it onto the PMU, its counters are stalled. \
                     Possible causes: more events configured in one group than the CPU has hardware counters, or \
                     tools holding the PMU system-wide.",
                    self.target,
                );
            }
            Interval::Starved => (),
        }
        Ok(())
    }

    /// The cumulative value of a counter of this group (corrected for multiplexing when `auto_scale`
    /// is enabled), as of the last [`read`](Self::read).
    #[allow(dead_code, reason = "only used by other plugins, once the module is public")]
    pub fn value(&self, id: CounterId) -> u64 {
        self.scaling.corrected()[id.0]
    }

    /// The cumulative values of every counter, in [`add`](EventGroupBuilder::add) order. See [`value`](Self::value).
    pub fn values(&self) -> &[u64] {
        self.scaling.corrected()
    }

    pub fn accuracy(&self) -> Accuracy {
        self.scaling.accuracy()
    }
}

/// Checks that `spec` (unified syntax) can be counted on this machine: it must be encodable, and the
/// kernel must accept to open it.
///
/// A task-attached event is opened on the calling process, a system-wide one on the first cpu of its
/// PMU; nothing is counted. The error says which step failed, with the cause (e.g. a permission error
/// when `perf_event_paranoid` is too high: the event may then exist but cannot be checked).
pub fn check_event(spec: &str) -> anyhow::Result<()> {
    let events = event::parse(spec).with_context(|| format!("event '{spec}' cannot be encoded"))?;
    for e in &events {
        let target = match e.scope() {
            Scope::TaskAttached { .. } => Target::Process { pid: 0 },
            Scope::SystemWide { pmu, cpus } => Target::Cpu {
                cpu: *cpus
                    .first()
                    .with_context(|| format!("PMU {pmu} has an empty cpumask"))?,
            },
        };
        EventGroupBuilder::open(target, false)
            .and_then(|mut group| group.add(e))
            .with_context(|| format!("event '{spec}' cannot be opened"))?;
    }
    Ok(())
}

/// Attach the counter to the group's target (leader and members must share these settings).
fn attach_to<'t>(builder: &mut perf_event::Builder<'t>, target: &'t Target) {
    match target {
        Target::Process { pid } => {
            // PID_MAX_LIMIT is 2^22 on Linux, a pid always fits.
            let pid = i32::try_from(*pid).expect("pid should fit in an i32");
            builder.observe_pid(pid).any_cpu();
        }
        Target::Cgroup { fd, cpu } => {
            builder.observe_cgroup(fd).one_cpu(*cpu as usize);
        }
        Target::Cpu { cpu } => {
            builder.any_pid().one_cpu(*cpu as usize);
        }
    }
}

/// Clear the `exclude_*` bits that [`perf_event::Builder::new`] sets by default.
fn include_all_domains(builder: &mut perf_event::Builder<'_>) {
    builder.exclude_user(false).exclude_kernel(false).exclude_hv(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_event_rejects_an_unknown_event_at_encoding() {
        let err = check_event("DEFINITELY_NOT_A_REAL_EVENT_XYZ").unwrap_err();
        assert!(format!("{err:#}").contains("cannot be encoded"), "got: {err:#}");
    }

    #[test]
    fn check_event_on_a_native_event_never_fails_at_encoding() {
        // Opening depends on the machine (perf_event_paranoid, virtualization), encoding does not.
        if let Err(err) = check_event("INSTRUCTIONS#u") {
            assert!(format!("{err:#}").contains("cannot be opened"), "got: {err:#}");
        }
    }
}
