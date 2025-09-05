use alumet::{
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Metric type based on GPU energy consumption data.
    pub gpu_energy_consumption: TypedMetricId<f64>,
    /// Metric type based on GPU used GTT memory data.
    pub gpu_memory_gtt_usage: TypedMetricId<u64>,
    /// Metric type based on GPU used VRAM memory data.
    pub gpu_memory_vram_usage: TypedMetricId<u64>,
    /// Metric type based on GPU electric power consumption data.
    pub gpu_power_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU temperature data.
    pub gpu_temperatures: TypedMetricId<u64>,
    /// Metric type based on GPU socket voltage consumption data.
    pub gpu_voltage_consumption: TypedMetricId<f64>,
    /// Metric type based on VRAM GPU process memory usage data.
    pub process_memory_usage_vram: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            gpu_energy_consumption: alumet.create_metric::<f64>(
                "amd_gpu_energy_consumption",
                PrefixedUnit::milli(Unit::Joule),
                "Get GPU energy consumption in milli Joule",
            )?,
            gpu_memory_gtt_usage: alumet.create_metric::<u64>(
                "amd_gpu_memory_gtt_usage",
                Unit::Byte,
                "Get GPU used GTT memory in Byte",
            )?,
            gpu_memory_vram_usage: alumet.create_metric::<u64>(
                "amd_gpu_memory_vram_usage",
                Unit::Byte,
                "Get GPU used VRAM memory in Byte",
            )?,
            gpu_power_consumption: alumet.create_metric::<u64>(
                "amd_gpu_power_consumption",
                Unit::Watt,
                "Get GPU electric average power consumption in Watts",
            )?,
            gpu_temperatures: alumet.create_metric::<u64>(
                "amd_gpu_temperature",
                Unit::DegreeCelsius,
                "Get GPU temperature in °C",
            )?,
            gpu_voltage_consumption: alumet.create_metric::<f64>(
                "amd_gpu_voltage_consumption",
                PrefixedUnit::milli(Unit::Volt),
                "Get GPU voltage socket consumption in milli Volt",
            )?,
            process_memory_usage_vram: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage_vram",
                Unit::Byte,
                "Get process VRAM memory usage in Byte",
            )?,
        })
    }
}
