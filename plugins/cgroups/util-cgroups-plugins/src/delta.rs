use alumet::plugin::util::CounterDiff;

/// CounterDiff to compute the delta when it makes sense.
pub struct CpuDeltaCounters {
    pub usage: CounterDiff,
    pub user: CounterDiff,
    pub system: CounterDiff,
}

impl CpuDeltaCounters {
    pub fn reset(&mut self) {
        self.usage.reset();
        self.user.reset();
        self.system.reset();
    }
}

impl Default for CpuDeltaCounters {
    fn default() -> Self {
        Self {
            usage: CounterDiff::with_max_value(u64::MAX),
            user: CounterDiff::with_max_value(u64::MAX),
            system: CounterDiff::with_max_value(u64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alumet::plugin::util::CounterDiffUpdate;

    #[test]
    fn test_cpu_delta_counters() {
        let mut cpu_delta_counters = CpuDeltaCounters::default();

        assert_eq!(cpu_delta_counters.usage.update(30), CounterDiffUpdate::FirstTime);
        assert_eq!(cpu_delta_counters.user.update(20), CounterDiffUpdate::FirstTime);
        assert_eq!(cpu_delta_counters.system.update(10), CounterDiffUpdate::FirstTime);

        assert_eq!(cpu_delta_counters.usage.update(60), CounterDiffUpdate::Difference(30));
        assert_eq!(cpu_delta_counters.user.update(50), CounterDiffUpdate::Difference(30));
        assert_eq!(cpu_delta_counters.system.update(40), CounterDiffUpdate::Difference(30));

        cpu_delta_counters.reset();

        assert_eq!(cpu_delta_counters.usage.update(90), CounterDiffUpdate::FirstTime);
        assert_eq!(cpu_delta_counters.user.update(80), CounterDiffUpdate::FirstTime);
        assert_eq!(cpu_delta_counters.system.update(70), CounterDiffUpdate::FirstTime);
    }
}
