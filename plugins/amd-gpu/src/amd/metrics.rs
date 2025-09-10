use alumet::{
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Metric type based on GPU activity data.
    pub gpu_activity: TypedMetricId<u64>,
    /// Metric type based on GPU energy consumption data.
    pub gpu_energy_consumption: TypedMetricId<f64>,
    /// Metric type based on GPU used memories data.
    pub gpu_memories_usage: TypedMetricId<u64>,
    /// Metric type based on GPU electric power consumption data.
    pub gpu_power_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU temperature data.
    pub gpu_temperatures: TypedMetricId<u64>,
    /// Metric type based on GPU socket voltage consumption data.
    pub gpu_voltage_consumption: TypedMetricId<f64>,
    /// Metric type based on GPU process compute unit usage data.
    pub process_compute_unit_usage: TypedMetricId<u64>,
    /// Metric type based on GPU process VRAM memory usage data.
    pub process_memory_usage_vram: TypedMetricId<u64>,
    /// Metric type based on GPU process SDMA usage data.
    pub process_sdma_usage: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            gpu_activity: alumet.create_metric::<u64>(
                "amd_gpu_activity",
                Unit::Percent,
                "Get GPU activity utilization in percentage",
            )?,
            gpu_energy_consumption: alumet.create_metric::<f64>(
                "amd_gpu_energy_consumption",
                PrefixedUnit::milli(Unit::Joule),
                "Get GPU energy consumption in milli joule",
            )?,
            gpu_memories_usage: alumet.create_metric::<u64>(
                "amd_gpu_memories_usage",
                Unit::Byte,
                "Get GPU used memory in byte",
            )?,
            gpu_power_consumption: alumet.create_metric::<u64>(
                "amd_gpu_power_consumption",
                Unit::Watt,
                "Get GPU electric average power consumption in watt",
            )?,
            gpu_temperatures: alumet.create_metric::<u64>(
                "amd_gpu_temperature",
                Unit::DegreeCelsius,
                "Get GPU temperature in °C",
            )?,
            gpu_voltage_consumption: alumet.create_metric::<f64>(
                "amd_gpu_voltage_consumption",
                Unit::Volt,
                "Get GPU voltage socket consumption in milli volt",
            )?,
            process_compute_unit_usage: alumet.create_metric::<u64>(
                "amd_gpu_process_compute_unit_usage",
                Unit::Percent,
                "Get process compute unit usage in percent",
            )?,
            process_memory_usage_vram: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage_vram",
                Unit::Byte,
                "Get process VRAM memory usage in byte",
            )?,
            process_sdma_usage: alumet.create_metric::<u64>(
                "amd_gpu_process_sdma_usage",
                PrefixedUnit::micro(Unit::Second),
                "Get process SDMA usage in micro second",
            )?,
        })
    }
}
