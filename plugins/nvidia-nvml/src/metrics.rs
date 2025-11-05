use alumet::{
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct Metrics {
    /// Total electric energy consumed by GPU in mJ.
    pub total_energy_consumption: TypedMetricId<f64>,
    /// Electric energy consumption measured at a given time in mW.
    pub instant_power: TypedMetricId<u64>,
    /// GPU temperature in °C
    pub temperature_gpu: TypedMetricId<u64>,
    /// GPU rate utilization in percentage
    pub major_utilization_gpu: TypedMetricId<u64>,
    /// GPU memory utilization in percentage
    pub major_utilization_memory: TypedMetricId<u64>,
    /// GPU video decoding property in percentage.
    pub decoder_utilization: TypedMetricId<u64>,
    /// Get the current utilization and sampling size for the decoder in μs.
    pub decoder_sampling_period_us: TypedMetricId<u64>,
    /// GPU video encoding property in percentage.
    pub encoder_utilization: TypedMetricId<u64>,
    /// Get the current utilization and sampling size for the encoder in μs.
    pub encoder_sampling_period_us: TypedMetricId<u64>,
    /// Time consumed by the streaming multiprocessors of a GPU in percentage.
    pub sm_utilization: TypedMetricId<u64>,
    /// Relevant currently running computing processes data in percentage.
    pub running_compute_processes: TypedMetricId<u64>,
    /// Relevant currently running graphical processes data in percentage.
    pub running_graphics_processes: TypedMetricId<u64>,
}

impl Metrics {
    /// Creates new Alumet metrics for NVML measurements and stores their ids in a `Metrics` structure.
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        Ok(Self {
            total_energy_consumption: alumet.create_metric(
                "nvml_energy_consumption",
                PrefixedUnit::milli(Unit::Joule),
                "Energy consumption by the GPU (including memory) since the previous measurement",
            )?,
            instant_power: alumet.create_metric(
                "nvml_instant_power",
                PrefixedUnit::milli(Unit::Watt),
                "Instantaneous power of the GPU at the time of the measurement",
            )?,
            temperature_gpu: alumet.create_metric(
                "nvml_temperature_gpu",
                Unit::DegreeCelsius,
                "Instantaneous temperature of the GPU at the time of the measurement",
            )?,
            major_utilization_gpu: alumet.create_metric(
                "nvml_gpu_utilization",
                Unit::Percent,
                "GPU rate utilization",
            )?,
            decoder_sampling_period_us: alumet.create_metric(
                "nvml_decoder_sampling_period",
                PrefixedUnit::micro(Unit::Second),
                "Get the current utilization and sampling size for the decoder",
            )?,
            encoder_sampling_period_us: alumet.create_metric(
                "nvml_encoder_sampling_period",
                PrefixedUnit::micro(Unit::Second),
                "Get the current utilization and sampling size for the encoder",
            )?,
            running_compute_processes: alumet.create_metric(
                "nvml_n_compute_processes",
                Unit::Unity,
                "Number of compute processes running on the device",
            )?,
            running_graphics_processes: alumet.create_metric(
                "nvml_n_graphic_processes",
                Unit::Unity,
                "Number of graphic processes running on the device",
            )?,

            // device process-related measurements
            major_utilization_memory: alumet.create_metric(
                "nvml_memory_utilization",
                Unit::Percent,
                "Utilization of the GPU memory by the process",
            )?,
            decoder_utilization: alumet.create_metric(
                "nvml_decoder_utilization",
                Unit::Percent,
                "Utilization of the GPU video decoder by the process",
            )?,
            encoder_utilization: alumet.create_metric(
                "nvml_encoder_utilization",
                Unit::Percent,
                "Utilization of the GPU video encoder by the process",
            )?,
            sm_utilization: alumet.create_metric(
                "nvml_sm_utilization",
                Unit::Percent,
                "Utilization of the GPU streaming multiprocessors by the process",
            )?,
        })
    }
}
