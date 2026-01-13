//! Utilities for implementing plugins.

/// Computes the difference between each successive measurement.
///
/// # Correction of overflows
/// `CounterDiff` automatically detects and corrects overflows by
/// using the maximum value of the counter.
///
/// # Example
/// ```
/// use alumet::plugin::util::{CounterDiff, CounterDiffUpdate};
///
/// let mut counter = CounterDiff::with_max_value(u32::MAX.into());
///
/// // first update, the value is stored but no difference can be computed
/// let v0 = 1;
/// let diff = counter.update(v0);
/// assert!(matches!(diff, CounterDiffUpdate::FirstTime));
///
/// // second update, we can use the difference
/// let v1 = 123;
/// match counter.update(v1) {
///     CounterDiffUpdate::FirstTime => unreachable!("unreachable in this example"),
///     CounterDiffUpdate::Difference(diff) => println!("v1 - v0 = {diff}"), // 122
///     CounterDiffUpdate::CorrectedDifference(diff) => println!("overflow-corrected difference = {diff}")
/// }
/// ```
pub struct CounterDiff {
    pub max_value: u64,
    previous_value: Option<u64>,
}

/// Result of [`CounterDiff::update()`].
#[derive(PartialEq, Debug)]
pub enum CounterDiffUpdate {
    /// This is the first counter update, its value is not meaningful.
    FirstTime,
    /// Normal counter update, gives the difference between the current and the previous value.
    Difference(u64),
    /// Counter update with overflow correction, gives the corrected difference.
    /// It is impossible to know whether only one or more than one overflow occurred.
    CorrectedDifference(u64),
}

impl CounterDiff {
    /// Creates a new `CounterDiff` with a maximum value.
    pub fn with_max_value(max_value: u64) -> CounterDiff {
        CounterDiff {
            max_value,
            previous_value: None,
        }
    }

    /// Provides a new value and computes the difference with the previous value, if there is one.
    pub fn update(&mut self, new_value: u64) -> CounterDiffUpdate {
        debug_assert!(new_value <= self.max_value, "No value can be greater than max_value!");
        let res = match self.previous_value {
            Some(prev) => {
                if new_value < prev {
                    let diff = self.max_value - prev + new_value + 1;
                    CounterDiffUpdate::CorrectedDifference(diff)
                } else {
                    let diff = new_value - prev;
                    CounterDiffUpdate::Difference(diff)
                }
            }
            None => CounterDiffUpdate::FirstTime,
        };
        self.previous_value = Some(new_value);
        res
    }
}

impl CounterDiffUpdate {
    /// Returns the difference that has been computed (and potentially corrected), or `None`.
    pub fn difference(self) -> Option<u64> {
        match self {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(d) | CounterDiffUpdate::CorrectedDifference(d) => Some(d),
        }
    }
}

impl From<CounterDiffUpdate> for Option<u64> {
    fn from(diff: CounterDiffUpdate) -> Self {
        diff.difference()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_diff_update_difference() {
        let first = CounterDiffUpdate::FirstTime;
        assert_eq!(first.difference(), None);
        let diff = CounterDiffUpdate::Difference(10);
        assert_eq!(diff.difference(), Some(10));
        let corrected_diff = CounterDiffUpdate::CorrectedDifference(50);
        assert_eq!(corrected_diff.difference(), Some(50));
    }

    #[test]
    fn test_counter_diff_first() {
        let mut counter = CounterDiff::with_max_value(255);
        let diff = counter.update(12);
        assert_eq!(diff, CounterDiffUpdate::FirstTime);
    }

    #[test]
    fn test_counter_diff_update() {
        let mut counter = CounterDiff::with_max_value(255);
        let expectations = vec![
            (10, CounterDiffUpdate::FirstTime),
            (15, CounterDiffUpdate::Difference(5)),
            (45, CounterDiffUpdate::Difference(30)),
            (255, CounterDiffUpdate::Difference(210)),
            (0, CounterDiffUpdate::CorrectedDifference(1)),
            (10, CounterDiffUpdate::Difference(10)),
            (3, CounterDiffUpdate::CorrectedDifference(249)),
            (2, CounterDiffUpdate::CorrectedDifference(255)),
            (2, CounterDiffUpdate::Difference(0)),
        ];
        for (idx, expectation) in expectations.iter().enumerate() {
            let diff = counter.update(expectation.0);
            assert_eq!(
                diff, expectation.1,
                "Failed at index {idx}: input={}, expected={:?}, got={:?}",
                expectation.0, expectation.1, diff
            );
        }
    }

    #[test]
    #[should_panic(expected = "No value can be greater than max_value!")]
    #[cfg(debug_assertions)]
    fn test_counter_diff_over_max_value() {
        let mut counter = CounterDiff::with_max_value(255);
        let _ = counter.update(256);
    }
}
