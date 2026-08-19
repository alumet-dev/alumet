use alumet::{
    measurement::{AttributeValue, MeasurementType},
    metrics::TypedMetricId,
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

/// Contains common metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Total CPU usage time by the cgroup since last measurement.
    pub cpu_time_delta: TypedMetricId<u64>,
    /// The ratio of CPU used by the cgroup since last measurement
    pub cpu_percent: TypedMetricId<f64>,
    /// Memory currently used by the cgroup.
    pub memory_usage: TypedMetricId<u64>,
    /// Maximum memory available for the cgroup.
    pub memory_max: TypedMetricId<f64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: TypedMetricId<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub memory_file: TypedMetricId<u64>,
    /// Memory reserved for kernel operations.
    pub memory_kernel_stack: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: TypedMetricId<u64>,
    /// Current amount of swap used by the cgroup.
    pub memory_swap_current: TypedMetricId<u64>,
    /// Maximum amount of swap that the cgroup can use.
    pub memory_swap_max: TypedMetricId<f64>,
    /// Reclaimable kernel slab memory used by the cgroup.
    pub slab_reclaimable: TypedMetricId<u64>,
    /// Number of pages swapped into memory by the cgroup.
    pub pswpin: TypedMetricId<u64>,
    /// Number of pages swapped out of memory by the cgroup.
    pub pswpout: TypedMetricId<u64>,
    /// IO pressure: some total delta (at least one task stalled)
    pub io_pressure_some_total: TypedMetricId<u64>,
    /// IO pressure: full total delta (all tasks stalled)
    pub io_pressure_full_total: TypedMetricId<u64>,
}

/// Used by probes to configure how cgroup measurements will be mapped to Alumet measurement points.
#[derive(Clone)]
pub struct AugmentedMetric<T: MeasurementType> {
    pub metric: TypedMetricId<T>,
    pub attributes: Vec<(String, AttributeValue)>,
}

impl<T: MeasurementType<T = T>> AugmentedMetric<T> {
    pub fn simple(metric: TypedMetricId<T>) -> Self {
        Self {
            metric,
            attributes: Vec::new(),
        }
    }

    pub fn with_attributes(metric: TypedMetricId<T>, attributes: Vec<(String, AttributeValue)>) -> Self {
        Self { metric, attributes }
    }
}

/// Regroups all metrics and their additional attributes.
#[derive(Clone)]
pub struct AugmentedMetrics {
    /// Total CPU usage time by the cgroup since last measurement.
    pub cpu_time_delta: AugmentedMetric<u64>,
    /// The ratio of CPU used by the cgroup since last measurement
    pub cpu_percent: AugmentedMetric<f64>,
    /// Memory currently used by the cgroup.
    pub memory_usage: AugmentedMetric<u64>,
    /// Maximum memory available for the cgroup.
    pub memory_max: AugmentedMetric<f64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: AugmentedMetric<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub memory_file: AugmentedMetric<u64>,
    /// Memory reserved for kernel operations.
    pub memory_kernel_stack: AugmentedMetric<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: AugmentedMetric<u64>,
    /// Current amount of swap used by the cgroup.
    pub memory_swap_current: AugmentedMetric<u64>,
    /// Maximum amount of swap that the cgroup can use.
    pub memory_swap_max: AugmentedMetric<f64>,
    /// Reclaimable kernel slab memory used by the cgroup.
    pub slab_reclaimable: AugmentedMetric<u64>,
    /// Number of pages swapped into memory by the cgroup.
    pub pswpin: AugmentedMetric<u64>,
    /// Number of pages swapped out of memory by the cgroup.
    pub pswpout: AugmentedMetric<u64>,
    /// IO pressure: some total delta (at least one task stalled)
    pub io_pressure_some_total: AugmentedMetric<u64>,
    /// IO pressure: full total delta (all tasks stalled)
    pub io_pressure_full_total: AugmentedMetric<u64>,

    /// Common attributes, added to the points of all metrics.
    pub common_attrs: Vec<(String, AttributeValue)>,
}

impl Metrics {
    /// Create the metrics and register them in Alumet.
    pub fn create(alumet: &mut AlumetPluginStart) -> anyhow::Result<Self> {
        let cpu_time_delta = alumet.create_metric::<u64>(
            "cpu_time_delta",
            PrefixedUnit::nano(Unit::Second),
            "Time spent by the cgroup on the CPU since the previous measurement",
        )?;
        let cpu_percent = alumet.create_metric::<f64>(
            "cpu_percent",
            Unit::Percent,
            "Part of the CPU used by the cgroup since the previous measurement (all cores fully used = 100%)",
        )?;
        let memory_usage = alumet.create_metric::<u64>(
            "memory_usage",
            Unit::Byte,
            "The total amount of memory currently being used by the cgroup and its descendants (at least in cgroupv2).",
        )?;
        let memory_max =
            alumet.create_metric::<f64>("memory_max", Unit::Byte, "Maximum memory available for the cgroup")?;
        let memory_anonymous = alumet.create_metric::<u64>(
            "cgroup_memory_anonymous",
            Unit::Byte,
            "Amount of memory used in anonymous mappings",
        )?;
        let memory_file = alumet.create_metric::<u64>(
            "cgroup_memory_file",
            Unit::Byte,
            "Amount of memory used to cache filesystem data, including tmpfs and shared memory.",
        )?;
        let memory_kernel_stack = alumet.create_metric::<u64>(
            "cgroup_memory_kernel_stack",
            Unit::Byte,
            "Amount of memory allocated to kernel stacks.",
        )?;
        let memory_pagetables = alumet.create_metric::<u64>(
            "cgroup_memory_pagetables",
            Unit::Byte,
            "Amount of memory allocated for page tables (which map virtual addresses to physical addresses).",
        )?;
        let memory_swap_current = alumet.create_metric::<u64>(
            "cgroup_memory_swap_current",
            Unit::Byte,
            "Current swap usage of the cgroup.",
        )?;
        let memory_swap_max = alumet.create_metric::<f64>(
            "cgroup_memory_swap_max",
            Unit::Byte,
            "Maximum allowed swap usage for the cgroup.",
        )?;
        let slab_reclaimable = alumet.create_metric::<u64>(
            "cgroup_slab_reclaimable",
            Unit::Byte,
            "Amount of reclaimable kernel slab memory used by the cgroup.",
        )?;
        let pswpin = alumet.create_metric::<u64>(
            "cgroup_pswpin",
            Unit::Unity,
            "Number of pages swapped into memory by the cgroup.",
        )?;

        let pswpout = alumet.create_metric::<u64>(
            "cgroup_pswpout",
            Unit::Unity,
            "Number of pages swapped out of memory by the cgroup.",
        )?;
        let io_pressure_some_total = alumet.create_metric::<u64>(
            "io_pressure_some_total",
            PrefixedUnit::micro(Unit::Second),
            "IO pressure some total delta: time with at least one task stalled since previous measurement",
        )?;
        let io_pressure_full_total = alumet.create_metric::<u64>(
            "io_pressure_full_total",
            PrefixedUnit::micro(Unit::Second),
            "IO pressure full total delta: time with all tasks stalled since previous measurement",
        )?;
        Ok(Self {
            cpu_time_delta,
            cpu_percent,
            memory_usage,
            memory_max,
            memory_anonymous,
            memory_file,
            memory_kernel_stack,
            memory_swap_current,
            memory_swap_max,
            memory_pagetables,
            slab_reclaimable,
            pswpin,
            pswpout,
            io_pressure_some_total,
            io_pressure_full_total,
        })
    }
}

impl AugmentedMetrics {
    pub fn no_additional_attribute(metrics: &Metrics) -> Self {
        Self {
            cpu_time_delta: AugmentedMetric::simple(metrics.cpu_time_delta),
            cpu_percent: AugmentedMetric::simple(metrics.cpu_percent),
            memory_usage: AugmentedMetric::simple(metrics.memory_usage),
            memory_max: AugmentedMetric::simple(metrics.memory_max),
            memory_anonymous: AugmentedMetric::simple(metrics.memory_anonymous),
            memory_file: AugmentedMetric::simple(metrics.memory_file),
            memory_kernel_stack: AugmentedMetric::simple(metrics.memory_kernel_stack),
            memory_pagetables: AugmentedMetric::simple(metrics.memory_pagetables),
            memory_swap_current: AugmentedMetric::simple(metrics.memory_swap_current),
            memory_swap_max: AugmentedMetric::simple(metrics.memory_swap_max),
            slab_reclaimable: AugmentedMetric::simple(metrics.slab_reclaimable),
            pswpin: AugmentedMetric::simple(metrics.pswpin),
            pswpout: AugmentedMetric::simple(metrics.pswpout),
            io_pressure_full_total: AugmentedMetric::simple(metrics.io_pressure_full_total),
            io_pressure_some_total: AugmentedMetric::simple(metrics.io_pressure_some_total),
            common_attrs: Vec::new(),
        }
    }

    pub fn with_common_attr_slice(
        metrics: &Metrics,
        common_attrs: &[(impl ToOwned<Owned = impl Into<String>>, AttributeValue)],
    ) -> Self {
        Self::with_common_attr_vec(
            metrics,
            common_attrs
                .iter()
                .map(|(k, v)| (k.to_owned().into(), v.to_owned()))
                .collect(),
        )
    }

    pub fn with_common_attr_vec(metrics: &Metrics, common_attrs: Vec<(String, AttributeValue)>) -> Self {
        Self {
            cpu_time_delta: AugmentedMetric::simple(metrics.cpu_time_delta),
            cpu_percent: AugmentedMetric::simple(metrics.cpu_percent),
            memory_usage: AugmentedMetric::simple(metrics.memory_usage),
            memory_max: AugmentedMetric::simple(metrics.memory_max),
            memory_anonymous: AugmentedMetric::simple(metrics.memory_anonymous),
            memory_file: AugmentedMetric::simple(metrics.memory_file),
            memory_kernel_stack: AugmentedMetric::simple(metrics.memory_kernel_stack),
            memory_pagetables: AugmentedMetric::simple(metrics.memory_pagetables),
            memory_swap_current: AugmentedMetric::simple(metrics.memory_swap_current),
            memory_swap_max: AugmentedMetric::simple(metrics.memory_swap_max),
            slab_reclaimable: AugmentedMetric::simple(metrics.slab_reclaimable),
            pswpin: AugmentedMetric::simple(metrics.pswpin),
            pswpout: AugmentedMetric::simple(metrics.pswpout),
            io_pressure_full_total: AugmentedMetric::simple(metrics.io_pressure_full_total),
            io_pressure_some_total: AugmentedMetric::simple(metrics.io_pressure_some_total),
            common_attrs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn augmented_metrics() {
        // just test that this compiles
        fn _f(metrics: &Metrics) {
            AugmentedMetrics::with_common_attr_slice(metrics, &[("".to_string(), AttributeValue::Bool(true))]);
            AugmentedMetrics::with_common_attr_slice(metrics, &[("", AttributeValue::Bool(true))]);
            AugmentedMetrics::with_common_attr_slice(metrics, vec![("", AttributeValue::Bool(true))].as_slice());
        }
    }
}
