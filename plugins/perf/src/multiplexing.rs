//! Compensation for perf counter multiplexing.
//!
//! A CPU only has a handful of hardware counters. When more events are requested than it can hold,
//! the kernel multiplexes them: a group is only on the PMU during a fraction `running / enabled` of
//! the time we asked it to count, and its raw values are underestimated by that same fraction. Like
//! the `perf` tool, we extrapolate the missing part by assuming that the events kept occurring at
//! the same rate while we were not looking.
//!
//! The correction is applied to each polling interval, not to the whole lifetime of the group, so
//! that an interval whose rate is overestimated cannot make the corrected value go backwards later
//! on: the metrics are counters, they must never decrease.

/// One read of a group.
///
/// The kernel reports cumulative values, never reset, hence the need to keep the previous read
/// around and to work on differences.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Snapshot {
    /// For how long we asked the group to count, in nanoseconds.
    ///
    /// Beware: this is *not* wall clock time. It only advances while the observed entity (process
    /// or cgroup) is actually running on a CPU. A sleeping process makes both times stand still.
    pub time_enabled: u128,
    /// For how long the group was really on the PMU, in nanoseconds.
    pub time_running: u128,
    /// Raw counter values, in the same order as the group's counters.
    pub values: Vec<u64>,
}

/// How faithful a reported (cumulative) value is.
///
/// This describes the whole value reported so far, not the last interval, because the value is a
/// cumulative counter. It therefore only ever degrades (`exact` → `extrapolated` → `underestimated`)
/// and never improves: a single imperfect interval taints every value reported afterwards. The
/// variants are ordered from best to worst so that [`Accuracy::max`] keeps the worst seen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Accuracy {
    /// Every interval was counted exactly.
    #[default]
    Exact,
    /// At least one multiplexed interval was extrapolated (only in `auto_scale` mode). The value is
    /// an estimate, which may be slightly above or below the truth.
    Extrapolated,
    /// The value is known to be too low: either multiplexed intervals were kept raw (no
    /// `auto_scale`), or some intervals were missed entirely (the group was starved).
    Underestimated,
}

impl Accuracy {
    /// The value used for the `accuracy` measurement attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            Accuracy::Exact => "exact",
            Accuracy::Extrapolated => "extrapolated",
            Accuracy::Underestimated => "underestimated",
        }
    }
}

/// What happened to a group during one polling interval.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Interval {
    /// The observed entity did not run at all: nothing happened, and nothing had to be counted.
    /// This is by far the most common case, hence the absence of any log.
    Idle,
    /// The group was on the PMU for the whole interval: the values are exact.
    Exact,
    /// The group was only on the PMU for part of the interval, so the raw values are underestimated
    /// by the ratio `running / enabled`.
    Multiplexed { running: u128, enabled: u128 },
    /// The entity ran but the group never made it onto the PMU: everything that happened during the
    /// interval was missed, and there is nothing to extrapolate from.
    Starved,
}

/// Multiplexing correction state of one group.
///
/// The kernel schedules a group atomically: either all of its counters are on the PMU, or none of
/// them are. Every counter of a group therefore shares the same `time_enabled`/`time_running`, and
/// a single correction applies to all of them.
#[derive(Debug, Default)]
pub(crate) struct Scaling {
    /// Previous read. Starts at zero: when the group is enabled it has counted nothing, during no
    /// time at all, so the first poll is an interval like any other.
    prev: Snapshot,
    /// Corrected cumulative value of each counter, in the same order as the group's counters.
    corrected: Vec<u64>,
    /// Faithfulness of the reported values so far. Only ever degrades, see [`Accuracy`].
    accuracy: Accuracy,
}

impl Scaling {
    pub fn new(n_counters: usize) -> Self {
        Self {
            prev: Snapshot {
                time_enabled: 0,
                time_running: 0,
                values: vec![0; n_counters],
            },
            corrected: vec![0; n_counters],
            accuracy: Accuracy::Exact,
        }
    }

    /// Corrected cumulative value of each counter, in the same order as the group's counters.
    pub fn corrected(&self) -> &[u64] {
        &self.corrected
    }

    /// Faithfulness of the reported values so far. See [`Accuracy`]: it only ever degrades.
    pub fn accuracy(&self) -> Accuracy {
        self.accuracy
    }

    /// Accounts for a new reading, updating the corrected totals, and returns what happened during
    /// the interval.
    pub fn account(&mut self, now: Snapshot, auto_scale: bool) -> Interval {
        let interval = correct_interval(&self.prev, &now, auto_scale, &mut self.corrected);
        let interval_accuracy = match interval {
            Interval::Idle | Interval::Exact => Accuracy::Exact,
            Interval::Multiplexed { .. } if auto_scale => Accuracy::Extrapolated,
            // multiplexed without auto-scaling, or a starved interval that could not be extrapolated
            Interval::Multiplexed { .. } | Interval::Starved => Accuracy::Underestimated,
        };
        self.accuracy = self.accuracy.max(interval_accuracy);
        self.prev = now;
        interval
    }
}

/// Adds the (possibly corrected) contribution of one polling interval to `corrected`, and reports
/// what happened. See the [module documentation](self) for the rationale.
fn correct_interval(prev: &Snapshot, now: &Snapshot, auto_scale: bool, corrected: &mut [u64]) -> Interval {
    let d_enabled = now.time_enabled.saturating_sub(prev.time_enabled);
    let d_running = now.time_running.saturating_sub(prev.time_running);

    if d_enabled == 0 {
        return Interval::Idle;
    }
    if d_running == 0 {
        // Nothing was counted, so there is nothing to extrapolate from. Leave the totals alone,
        // which keeps the counters flat instead of making up a value. Note that dividing by
        // `d_running` below would panic, integer division by zero is not `inf`.
        return Interval::Starved;
    }

    let multiplexed = d_running < d_enabled;
    for (i, total) in corrected.iter_mut().enumerate() {
        let delta = u128::from(now.values[i].saturating_sub(prev.values[i]));
        let contribution = if multiplexed && auto_scale {
            // Rule of three: `delta` events were counted during `d_running`, estimate how many
            // occurred during the whole `d_enabled`.
            delta * d_enabled / d_running
        } else {
            delta
        };
        *total = total.saturating_add(u64::try_from(contribution).unwrap_or(u64::MAX));
    }

    if multiplexed {
        Interval::Multiplexed {
            running: d_running,
            enabled: d_enabled,
        }
    } else {
        Interval::Exact
    }
}

#[cfg(test)]
mod tests {
    use super::{Interval, Snapshot, correct_interval};

    const SEC: u128 = 1_000_000_000;

    fn snap(enabled: u128, running: u128, values: &[u64]) -> Snapshot {
        Snapshot {
            time_enabled: enabled,
            time_running: running,
            values: values.to_vec(),
        }
    }

    /// When the group is enabled, everything is zero, so the first poll is an interval like any
    /// other and must be corrected too.
    #[test]
    fn first_poll_is_an_interval_like_any_other() {
        let mut corrected = vec![0];
        let interval = correct_interval(&snap(0, 0, &[0]), &snap(SEC, SEC / 4, &[1000]), true, &mut corrected);
        assert_eq!(
            interval,
            Interval::Multiplexed {
                running: SEC / 4,
                enabled: SEC
            }
        );
        // counted 1000 while on the PMU a quarter of the time
        assert_eq!(corrected, vec![4000]);
    }

    #[test]
    fn exact_when_running_equals_enabled() {
        let mut corrected = vec![0];
        let interval = correct_interval(&snap(0, 0, &[0]), &snap(SEC, SEC, &[1000]), true, &mut corrected);
        assert_eq!(interval, Interval::Exact);
        assert_eq!(corrected, vec![1000]);
    }

    /// The observed entity did not run: both times stand still. This is the most common case and it
    /// must not be mistaken for a starved group.
    #[test]
    fn idle_entity_changes_nothing() {
        let mut corrected = vec![1000];
        let prev = snap(SEC, SEC, &[1000]);
        let interval = correct_interval(&prev, &prev.clone(), true, &mut corrected);
        assert_eq!(interval, Interval::Idle);
        assert_eq!(corrected, vec![1000]);
    }

    /// The entity ran but the group never made it onto the PMU. Dividing by `d_running` here would
    /// panic, and there is nothing to extrapolate from anyway.
    #[test]
    fn starved_group_does_not_divide_by_zero() {
        let mut corrected = vec![1000];
        let prev = snap(SEC, SEC / 4, &[1000]);
        let now = snap(2 * SEC, SEC / 4, &[1000]);
        let interval = correct_interval(&prev, &now, true, &mut corrected);
        assert_eq!(interval, Interval::Starved);
        assert_eq!(corrected, vec![1000], "the total must stay flat, not be made up");
    }

    #[test]
    fn without_auto_scale_the_raw_delta_is_kept() {
        let mut corrected = vec![0];
        let interval = correct_interval(&snap(0, 0, &[0]), &snap(SEC, SEC / 4, &[1000]), false, &mut corrected);
        assert_eq!(
            interval,
            Interval::Multiplexed {
                running: SEC / 4,
                enabled: SEC
            },
            "multiplexing is still reported, it is just not compensated"
        );
        assert_eq!(corrected, vec![1000]);
    }

    #[test]
    fn every_counter_of_the_group_is_corrected() {
        let mut corrected = vec![0, 0, 0];
        correct_interval(&snap(0, 0, &[0, 0, 0]), &snap(SEC, SEC / 2, &[10, 20, 30]), true, &mut corrected);
        assert_eq!(corrected, vec![20, 40, 60]);
    }

    /// Each interval is corrected on its own and added to a total that only ever grows, so that a
    /// badly extrapolated interval cannot make the counter go backwards later on.
    #[test]
    fn totals_accumulate_and_never_decrease() {
        let mut corrected = vec![0];
        let mut totals = Vec::new();

        // poll 1: 1000 counted over a quarter of a second of PMU time -> 4000
        let s1 = snap(SEC, SEC / 4, &[1000]);
        correct_interval(&snap(0, 0, &[0]), &s1, true, &mut corrected);
        totals.push(corrected[0]);

        // poll 2: +500 counted over another quarter -> +2000
        let s2 = snap(2 * SEC, SEC / 2, &[1500]);
        correct_interval(&s1, &s2, true, &mut corrected);
        totals.push(corrected[0]);

        // poll 3: the group is starved, nothing is added
        let s3 = snap(3 * SEC, SEC / 2, &[1500]);
        correct_interval(&s2, &s3, true, &mut corrected);
        totals.push(corrected[0]);

        // poll 4: +200 counted over half a second -> +400
        let s4 = snap(4 * SEC, SEC, &[1700]);
        correct_interval(&s3, &s4, true, &mut corrected);
        totals.push(corrected[0]);

        assert_eq!(totals, vec![4000, 6000, 6000, 6400]);
        assert!(totals.windows(2).all(|w| w[1] >= w[0]), "a counter must never decrease");
    }

    /// `account` reports each interval on its own; the caller is the one tracking streaks.
    #[test]
    fn account_reports_the_interval_and_degrades_accuracy() {
        use super::{Accuracy, Interval, Scaling};
        let mut scaling = Scaling::new(1);

        assert_eq!(scaling.account(snap(SEC, SEC, &[10]), true), Interval::Exact);
        // a starved interval is reported as such every time it happens, without any streak state
        assert_eq!(scaling.account(snap(2 * SEC, SEC, &[10]), true), Interval::Starved);
        assert_eq!(scaling.account(snap(3 * SEC, SEC, &[10]), true), Interval::Starved);
        assert_eq!(
            scaling.accuracy(),
            Accuracy::Underestimated,
            "a starved interval misses data, so the total is underestimated"
        );
    }

    /// The accuracy only ever degrades, and a starvation makes it worse than a mere extrapolation.
    #[test]
    fn accuracy_degrades_to_the_worst_interval() {
        use super::{Accuracy, Scaling};

        let mut scaling = Scaling::new(1);
        assert_eq!(scaling.accuracy(), Accuracy::Exact);

        scaling.account(snap(SEC, SEC, &[10]), true); // exact
        assert_eq!(scaling.accuracy(), Accuracy::Exact);

        scaling.account(snap(2 * SEC, SEC + SEC / 2, &[20]), true); // multiplexed, auto-scaled
        assert_eq!(scaling.accuracy(), Accuracy::Extrapolated);

        scaling.account(snap(3 * SEC, SEC + SEC / 2, &[20]), true); // starved
        assert_eq!(scaling.accuracy(), Accuracy::Underestimated);

        scaling.account(snap(4 * SEC, 2 * SEC + SEC / 2, &[30]), true); // exact again
        assert_eq!(scaling.accuracy(), Accuracy::Underestimated, "accuracy never improves");
    }

    /// Without auto-scaling, a multiplexed interval is reported raw, hence underestimated.
    #[test]
    fn accuracy_is_underestimated_without_auto_scale() {
        use super::{Accuracy, Scaling};

        let mut scaling = Scaling::new(1);
        scaling.account(snap(SEC, SEC / 2, &[10]), false); // multiplexed, not scaled
        assert_eq!(scaling.accuracy(), Accuracy::Underestimated);
    }
}
