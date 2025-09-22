use alumet::{
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Metric type based on GPU activity usage data.
    pub gpu_activity_usage: TypedMetricId<f64>,
    /// Metric type based on GPU energy consumption data.
    pub gpu_energy_consumption: TypedMetricId<f64>,
    /// Metric type based on GPU used memory data.
    pub gpu_memory_usages: TypedMetricId<u64>,
    /// Metric type based on GPU electric power consumption data.
    pub gpu_power_consumption: TypedMetricId<u64>,
    /// Metric type based on GPU temperature data.
    pub gpu_temperatures: TypedMetricId<u64>,
    /// Metric type based on GPU socket voltage data.
    pub gpu_voltage: TypedMetricId<u64>,
    /// Metric type based on GPU process memory usage data.
    pub process_memory_usage: TypedMetricId<u64>,
    /// Metric type based on GPU GFX engine usage data.
    pub process_engine_usage_gfx: TypedMetricId<u64>,
    /// Metric type based on GPU encode engine usage data.
    pub process_engine_usage_encode: TypedMetricId<u64>,
    /// Metric type based on GTT GPU process memory usage data.
    pub process_memory_usage_gtt: TypedMetricId<u64>,
    /// Metric type based on CPU GPU process memory usage data.
    pub process_memory_usage_cpu: TypedMetricId<u64>,
    /// Metric type based on VRAM GPU process memory usage data.
    pub process_memory_usage_vram: TypedMetricId<u64>,
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            gpu_activity_usage: alumet.create_metric::<f64>(
                "amd_gpu_activity_usage",
                Unit::Percent,
                "Get GPU activity usage in percentage",
            )?,
            gpu_energy_consumption: alumet.create_metric::<f64>(
                "amd_gpu_energy_consumption",
                PrefixedUnit::milli(Unit::Joule),
                "Get GPU energy consumption in millijoule",
            )?,
            gpu_memory_usages: alumet.create_metric::<u64>(
                "amd_gpu_memory_usage",
                Unit::Byte,
                "Get GPU used memory in byte",
            )?,
            gpu_power_consumption: alumet.create_metric::<u64>(
                "amd_gpu_power_consumption",
                Unit::Watt,
                "Get GPU electric average power consumption in watts",
            )?,
            gpu_temperatures: alumet.create_metric::<u64>(
                "amd_gpu_temperature",
                Unit::DegreeCelsius,
                "Get GPU temperature in °C",
            )?,
            gpu_voltage: alumet.create_metric::<u64>(
                "amd_gpu_voltage",
                PrefixedUnit::milli(Unit::Volt),
                "Get GPU voltage in millivolt",
            )?,
            process_memory_usage: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage",
                Unit::Byte,
                "Get process memory usage in byte",
            )?,
            process_engine_usage_encode: alumet.create_metric::<u64>(
                "amd_gpu_process_engine_usage_encode",
                PrefixedUnit::nano(Unit::Second),
                "Get process encode engine usage in nanosecond",
            )?,
            process_engine_usage_gfx: alumet.create_metric::<u64>(
                "amd_gpu_process_engine_gfx",
                PrefixedUnit::nano(Unit::Second),
                "Get process GFX engine usage in nanosecond",
            )?,
            process_memory_usage_gtt: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage_gtt",
                Unit::Byte,
                "Get process GTT memory usage in byte",
            )?,
            process_memory_usage_cpu: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage_cpu",
                Unit::Byte,
                "Get process CPU memory usage in byte",
            )?,
            process_memory_usage_vram: alumet.create_metric::<u64>(
                "amd_gpu_process_memory_usage_vram",
                Unit::Byte,
                "Get process VRAM memory usage in byte",
            )?,
        })
    }
}
