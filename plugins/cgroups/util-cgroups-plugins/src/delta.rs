use alumet::plugin::util::CounterDiff;

/// CounterDiff to compute the delta when it makes sense.
pub struct CpuDeltaCounters {
    pub usage: CounterDiff,
    pub user: CounterDiff,
    pub system: CounterDiff,
}

/// CounterDiff to compute the delta when it makes sense.
pub struct IoDeltaCounters {
    pub some: CounterDiff,
    pub full: CounterDiff,
}

/// CounterDiff to compute the delta when it makes sense.
pub struct MemoryDeltaCounters {
    pub page_in: CounterDiff,
    pub page_out: CounterDiff,
}

impl CpuDeltaCounters {
    pub fn reset(&mut self) {
        self.usage.reset();
        self.user.reset();
        self.system.reset();
    }
}

impl IoDeltaCounters {
    pub fn reset(&mut self) {
        self.some.reset();
        self.full.reset();
    }
}

impl MemoryDeltaCounters {
    pub fn reset(&mut self) {
        self.page_in.reset();
        self.page_out.reset();
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

impl Default for IoDeltaCounters {
    fn default() -> Self {
        Self {
            some: CounterDiff::with_max_value(u64::MAX),
            full: CounterDiff::with_max_value(u64::MAX),
        }
    }
}

impl Default for MemoryDeltaCounters {
    fn default() -> Self {
        Self {
            page_in: CounterDiff::with_max_value(u64::MAX),
            page_out: CounterDiff::with_max_value(u64::MAX),
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

    #[test]
    fn test_io_delta_counters() {
        let mut io_delta_counters = IoDeltaCounters::default();

        assert_eq!(io_delta_counters.full.update(800), CounterDiffUpdate::FirstTime);
        assert_eq!(io_delta_counters.some.update(75), CounterDiffUpdate::FirstTime);

        assert_eq!(io_delta_counters.full.update(860), CounterDiffUpdate::Difference(60));
        assert_eq!(io_delta_counters.some.update(75), CounterDiffUpdate::Difference(0));

        io_delta_counters.reset();

        assert_eq!(io_delta_counters.full.update(20), CounterDiffUpdate::FirstTime);
        assert_eq!(io_delta_counters.some.update(80), CounterDiffUpdate::FirstTime);
    }

    #[test]
    fn test_mem_delta_counters() {
        let mut memory_delta_counters = MemoryDeltaCounters::default();

        assert_eq!(memory_delta_counters.page_in.update(800), CounterDiffUpdate::FirstTime);
        assert_eq!(memory_delta_counters.page_out.update(75), CounterDiffUpdate::FirstTime);

        assert_eq!(
            memory_delta_counters.page_in.update(860),
            CounterDiffUpdate::Difference(60)
        );
        assert_eq!(
            memory_delta_counters.page_out.update(75),
            CounterDiffUpdate::Difference(0)
        );

        memory_delta_counters.reset();

        assert_eq!(memory_delta_counters.page_in.update(20), CounterDiffUpdate::FirstTime);
        assert_eq!(memory_delta_counters.page_out.update(80), CounterDiffUpdate::FirstTime);
    }
}
