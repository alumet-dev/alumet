use alumet::{
    metrics::{error::MetricCreationError, TypedMetricId},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

#[derive(Debug, Clone)]
pub struct Metrics {
    pub cpu_metric: TypedMetricId<u64>,
    pub memory_metric: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let usec = PrefixedUnit::micro(Unit::Second);

        Ok(Self {
            cpu_metric: alumet.create_metric::<u64>(
                "cpu_time",
                usec,
                "Total CPU time consumed by the cgroup (in nanoseconds).",
            )?,
            memory_metric: alumet.create_metric::<u64>(
                "memory_usage",
                Unit::Unity,
                "Total memory usage by the cgroup (in bytes).",
            )?,
        })
    }
}
