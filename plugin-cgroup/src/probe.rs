use std::u64;

use alumet::{
    measurement::{AttributeValue, MeasurementType},
    metrics::TypedMetricId,
    plugin::{util::CounterDiff, AlumetPluginStart},
    units::{PrefixedUnit, Unit},
};

/// Notification-based creator of the cgroup probes, v1 and v2.
pub mod creator;
/// Probe for cgroups v1.
pub mod v1;
/// Probe for cgroups v2.
pub mod v2;

pub mod personalise;

/// CounterDiff to compute the delta when it makes sense.
struct DeltaCounters {
    usage: CounterDiff,
    user: CounterDiff,
    system: CounterDiff,
}

impl Default for DeltaCounters {
    fn default() -> Self {
        Self {
            usage: CounterDiff::with_max_value(u64::MAX),
            user: CounterDiff::with_max_value(u64::MAX),
            system: CounterDiff::with_max_value(u64::MAX),
        }
    }
}

/// Contains common metrics.
#[derive(Clone, Eq, PartialEq)]
pub struct Metrics {
    /// Total CPU usage time by the cgroup since last measurement.
    pub cpu_time_delta: TypedMetricId<u64>,
    /// Memory currently used by the cgroup.
    pub memory_usage: TypedMetricId<u64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: TypedMetricId<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub memory_file: TypedMetricId<u64>,
    /// Memory reserved for kernel operations.
    pub memory_kernel_stack: TypedMetricId<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: TypedMetricId<u64>,
}

/// Used by probes to configure how cgroup measurements will be mapped to Alumet measurement points.
#[derive(Clone, PartialEq, Eq)]
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
#[derive(Clone, Eq, PartialEq)]
pub struct AugmentedMetrics {
    /// Total CPU usage time by the cgroup since last measurement.
    pub cpu_time_delta: AugmentedMetric<u64>,
    /// Memory currently used by the cgroup.
    pub memory_usage: AugmentedMetric<u64>,
    /// Anonymous used memory, corresponding to running process and various allocated memory.
    pub memory_anonymous: AugmentedMetric<u64>,
    /// Files memory, corresponding to open files and descriptors.
    pub memory_file: AugmentedMetric<u64>,
    /// Memory reserved for kernel operations.
    pub memory_kernel_stack: AugmentedMetric<u64>,
    /// Memory used to manage correspondence between virtual and physical addresses.
    pub memory_pagetables: AugmentedMetric<u64>,

    /// Common attributes, added to the points of all metrics.
    pub common_attrs: Vec<(String, AttributeValue)>,
}

impl Metrics {
    /// Create the metrics and register them in Alumet.
    pub fn create(alumet: &mut AlumetPluginStart) -> anyhow::Result<Self> {
        let cpu_time_delta = alumet.create_metric::<u64>(
            "cpu_time_delta",
            PrefixedUnit::nano(Unit::Second),
            "Total CPU usage time by the cgroup since last measurement",
        )?;
        let memory_usage =
            alumet.create_metric::<u64>("memory_usage", Unit::Byte, "Memory currently used by the cgroup")?;
        let memory_anonymous = alumet.create_metric::<u64>(
            "cgroup_memory_anonymous",
            Unit::Byte,
            "Anonymous used memory, corresponding to running process and various allocated memory",
        )?;
        let memory_file = alumet.create_metric::<u64>(
            "cgroup_memory_file",
            Unit::Byte,
            "Files memory, corresponding to open files and descriptors",
        )?;
        let memory_kernel_stack = alumet.create_metric::<u64>(
            "cgroup_memory_kernel_stack",
            Unit::Byte,
            "Memory reserved for kernel operations",
        )?;
        let memory_pagetables = alumet.create_metric::<u64>(
            "cgroup_memory_pagetables",
            Unit::Byte,
            "Memory used to manage correspondence between virtual and physical addresses",
        )?;
        Ok(Self {
            cpu_time_delta,
            memory_usage,
            memory_anonymous,
            memory_file,
            memory_kernel_stack,
            memory_pagetables,
        })
    }
}

impl AugmentedMetrics {
    pub fn no_additional_attribute(metrics: &Metrics) -> Self {
        Self {
            cpu_time_delta: AugmentedMetric::simple(metrics.cpu_time_delta),
            memory_usage: AugmentedMetric::simple(metrics.memory_usage),
            memory_anonymous: AugmentedMetric::simple(metrics.memory_anonymous),
            memory_file: AugmentedMetric::simple(metrics.memory_file),
            memory_kernel_stack: AugmentedMetric::simple(metrics.memory_kernel_stack),
            memory_pagetables: AugmentedMetric::simple(metrics.memory_pagetables),
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
                .into_iter()
                .map(|(k, v)| (k.to_owned().into(), v.to_owned()))
                .collect(),
        )
    }

    pub fn with_common_attr_vec(metrics: &Metrics, common_attrs: Vec<(String, AttributeValue)>) -> Self {
        Self {
            cpu_time_delta: AugmentedMetric::simple(metrics.cpu_time_delta),
            memory_usage: AugmentedMetric::simple(metrics.memory_usage),
            memory_anonymous: AugmentedMetric::simple(metrics.memory_anonymous),
            memory_file: AugmentedMetric::simple(metrics.memory_file),
            memory_kernel_stack: AugmentedMetric::simple(metrics.memory_kernel_stack),
            memory_pagetables: AugmentedMetric::simple(metrics.memory_pagetables),
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
