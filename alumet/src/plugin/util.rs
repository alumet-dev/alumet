//! Utilities for implementing plugins.

pub struct CounterDiff {
    pub max_value: u64,
    previous_value: Option<u64>,
}

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
    pub fn with_max_value(max_value: u64) -> CounterDiff {
        CounterDiff {
            max_value,
            previous_value: None,
        }
    }

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
