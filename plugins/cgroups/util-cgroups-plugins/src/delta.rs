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
