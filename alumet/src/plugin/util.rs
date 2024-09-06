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
///     CounterDiffUpdate::CorrectedDifference(diff) => println!("overflow-corrected diffrence = {diff}")
/// }
/// ```
pub struct CounterDiff {
    pub max_value: u64,
    previous_value: Option<u64>,
}

/// Result of [`CounterDiff::update()`].
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
                    let diff = new_value - prev + self.max_value;
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
