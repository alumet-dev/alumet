//! Compensation for perf counter multiplexing.
//!
//! A CPU only has a handful of hardware counters. When more events are requested than it can hold,
//! the kernel multiplexes them: a group is only on the PMU during a fraction `running / enabled` of
//! the time we asked it to count, and its raw values are underestimated by that same fraction.
//!
//! The correction is applied to the whole lifetime of the group (`raw * enabled / running` on the
//! cumulative values). The estimate can go down a little between two polls: the reported values are
//! not monotonic counters.

/// How faithful the values of a group are, from its cumulative times.
///
/// There are in fact **two independent axes**, both starting from `Exact`:
/// - real multiplexing on a single-PMU event: `Exact` -> `Extrapolated` (with `auto_scale`) or
///   `Underestimated`;
/// - a partial-pmu event on a hybrid CPU: `Exact` -> `Partial`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Accuracy {
    /// The group has been on the PMU for its whole lifetime.
    #[default]
    Exact,
    /// The value is a per-pmu count from a hybrid CPU: the event is pinned to one pmu and the
    /// entity also ran on others, so it was on the PMU only part of the time. The value is *not*
    /// scaled.
    Partial,
    /// The group was multiplexed and its values are extrapolated (only in `auto_scale` mode). The
    /// value is an estimate, which may be slightly above or below the truth.
    Extrapolated,
    /// The value is known to be too low: either the group was multiplexed and its values are kept
    /// raw (no `auto_scale`), or it has never been on the PMU (starved).
    Underestimated,
}

impl Accuracy {
    /// The value used for the `accuracy` measurement attribute.
    pub fn as_str(self) -> &'static str {
        match self {
            Accuracy::Exact => "exact",
            Accuracy::Partial => "partial",
            Accuracy::Extrapolated => "extrapolated",
            Accuracy::Underestimated => "underestimated",
        }
    }
}

/// Estimates the value of each counter of a group from a read of the group (`raw`, `enabled` and
/// `running` are cumulative, as reported by the kernel), and tells how faithful they are.
pub(crate) fn estimate(
    raw: &[u64],
    enabled: u128,
    running: u128,
    auto_scale: bool,
    partial_pmu: bool,
) -> (Vec<u64>, Accuracy) {
    let accuracy = if running == enabled {
        Accuracy::Exact
    } else if partial_pmu {
        Accuracy::Partial
    } else if auto_scale && running > 0 {
        Accuracy::Extrapolated
    } else {
        Accuracy::Underestimated
    };
    let values = raw
        .iter()
        .map(|&raw| match accuracy {
            // Rule of three: `raw` events were counted during `running`, estimate how many
            // occurred during the whole `enabled`.
            Accuracy::Extrapolated => u64::try_from(u128::from(raw) * enabled / running).unwrap_or(u64::MAX),
            _ => raw,
        })
        .collect();
    (values, accuracy)
}

#[cfg(test)]
mod tests {
    use super::{Accuracy, estimate};

    const SEC: u128 = 1_000_000_000;

    #[test]
    fn exact_when_always_on_the_pmu() {
        assert_eq!(estimate(&[1000], SEC, SEC, true, false), (vec![1000], Accuracy::Exact));
    }

    /// When the group is enabled, nothing has been counted, during no time at all.
    #[test]
    fn exact_before_anything_ran() {
        assert_eq!(estimate(&[0], 0, 0, true, false), (vec![0], Accuracy::Exact));
    }

    #[test]
    fn multiplexed_values_are_extrapolated() {
        // counted 1000 while on the PMU a quarter of the time
        assert_eq!(
            estimate(&[1000], SEC, SEC / 4, true, false),
            (vec![4000], Accuracy::Extrapolated)
        );
    }

    #[test]
    fn every_counter_of_the_group_is_extrapolated() {
        assert_eq!(
            estimate(&[10, 20, 30], SEC, SEC / 2, true, false),
            (vec![20, 40, 60], Accuracy::Extrapolated)
        );
    }

    #[test]
    fn multiplexed_values_are_kept_raw_without_auto_scale() {
        assert_eq!(
            estimate(&[1000], SEC, SEC / 4, false, false),
            (vec![1000], Accuracy::Underestimated)
        );
    }

    /// The entity ran but the group has never been on the PMU: there is nothing to extrapolate
    /// from, and dividing by `running` would panic.
    #[test]
    fn starved_group_is_underestimated() {
        assert_eq!(estimate(&[0], SEC, 0, true, false), (vec![0], Accuracy::Underestimated));
    }

    /// Once a starved group makes it onto the PMU, the cumulative ratio also covers the starved time.
    #[test]
    fn starved_group_is_extrapolated_once_it_ran() {
        assert_eq!(
            estimate(&[1000], 2 * SEC, SEC / 2, true, false),
            (vec![4000], Accuracy::Extrapolated)
        );
    }

    /// A partial-pmu group only ran part of the time (the entity was on another pmu): its raw value
    /// is exact for this pmu, so it is kept as-is and never scaled.
    #[test]
    fn partial_pmu_values_are_not_scaled() {
        assert_eq!(
            estimate(&[1000], SEC, SEC / 4, true, true),
            (vec![1000], Accuracy::Partial)
        );
        assert_eq!(estimate(&[1000], SEC, SEC, true, true), (vec![1000], Accuracy::Exact));
    }

    /// The rule of three is applied to the cumulative values: when the group spends more of its
    /// time on the PMU, the estimate goes down.
    #[test]
    fn estimate_can_go_down() {
        assert_eq!(estimate(&[1000], SEC, SEC / 4, true, false).0, vec![4000]);
        assert_eq!(estimate(&[1500], 2 * SEC, SEC, true, false).0, vec![3000]);
    }
}
