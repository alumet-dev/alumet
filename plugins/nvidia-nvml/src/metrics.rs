use std::collections::HashMap;

use alumet::{
    measurement::AttributeValue,
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};
use nvml_wrapper::enums::gpm::GpmMetricId::{self};

/// Contains the ids of the measured metrics.
#[derive(Clone)]
pub struct FullMetrics {
    /// Total electric energy consumed by GPU in mJ.
    pub total_energy_consumption: TypedMetricId<f64>,
    /// Electric energy consumption measured at a given time in μJ.
    pub instant_power: TypedMetricId<u64>,
    /// GPU temperature in °C
    pub temperature_gpu: TypedMetricId<u64>,
    /// GPU rate utilization in percentage
    pub major_utilization_gpu: TypedMetricId<u64>,
    /// GPU memory utilization in percentage
    pub major_utilization_memory: TypedMetricId<u64>,
    /// Used, free and total VRAM memory, in bytes.
    pub memory_info: TypedMetricId<u64>,
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
    // Amount of used GPU memory in bytes
    pub used_gpu_memory: TypedMetricId<u64>,
    /// Get the current clock frequency in Hertz
    pub clock_info: TypedMetricId<u64>,
    /// Current power cap (power management limit) in mW.
    pub power_limit: TypedMetricId<u64>,

    pub gpm_metrics: HashMap<GpmMetricId, TypedMetricId<u64>>,
}

#[derive(Clone)]
pub struct MinimalMetrics {
    /// Total electric energy consumed by GPU in mJ (estimated from the power).
    pub total_energy_consumption: TypedMetricId<f64>,
    /// Electric energy consumption measured at a given time in μJ.
    pub instant_power: TypedMetricId<u64>,
}

impl FullMetrics {
    /// Creates new Alumet metrics for NVML measurements and stores their ids in a `Metrics` structure.
    pub fn new(
        alumet: &mut AlumetPluginStart,
        gpm_metrics_requested: &Vec<GpmMetricId>,
    ) -> Result<Self, MetricCreationError> {
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
            memory_info: alumet.create_metric(
                "nvml_gpu_memory_info",
                Unit::Byte,
                "VRAM information (free, used, reserved and total size)",
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
            used_gpu_memory: alumet.create_metric("nvml_used_gpu_memory", Unit::Byte, "Amount of used GPU memory")?,

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
            clock_info: alumet.create_metric("nvml_clock_info", Unit::Hertz, "Current clock speed for the device")?,
            power_limit: alumet.create_metric(
                "nvml_power_limit",
                PrefixedUnit::milli(Unit::Watt),
                "Current power cap (power management limit) of the GPU",
            )?,
            gpm_metrics: add_gpm_metrics(gpm_metrics_requested, alumet),
        })
    }

    /// Returns the list of keys in `gpm_metrics` corresponding to the list of GPM metrics requested.
    pub fn gpm_metrics_ids(&self) -> Vec<GpmMetricId> {
        self.gpm_metrics.keys().map(|key| key.to_owned()).collect()
    }
}

impl MinimalMetrics {
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
        })
    }
}

/// Adds every GPM metric requested to [AlumetPluginStart].
fn add_gpm_metrics(
    gpm_metrics_requested: &Vec<GpmMetricId>,
    alumet: &mut AlumetPluginStart,
) -> HashMap<GpmMetricId, TypedMetricId<u64>> {
    let mut hash_map = HashMap::new();
    for gpm_metric in gpm_metrics_requested {
        match create_gpm_metrics(gpm_metric, alumet) {
            Ok(result) => {
                hash_map.insert(*gpm_metric, result);
            }
            _ => continue,
        }
    }
    hash_map
}

/// Create a metric from the the corresponding [GpmMetricId], with the right unit, name, and description.
///
/// Multiple [GpmMetricId] may map to the same metric (for example : [GpmMetricId::Nvdec0Util] [...] [GpmMetricId::Nvdec7Util] map to
/// metric `nvml_gpm_nvdec_utilization`)  
fn create_gpm_metrics(
    gpm_metric: &GpmMetricId,
    alumet: &mut AlumetPluginStart,
) -> Result<TypedMetricId<u64>, MetricCreationError> {
    Ok(match gpm_metric {
        // Percentage of time any warp was active on a multiprocessor, averaged over all multiprocessors.
        GpmMetricId::GraphicsUtil => alumet.create_metric(
            "nvml_gpm_graphics_util",
            Unit::Percent,
            "Percentage of time any warp was active on a multiprocessor, averaged over all multiprocessors.",
        )?,

        // Percentage of time each multiprocessor had at least 1 warp assigned, averaged over all multiprocessors.
        GpmMetricId::SmUtil => alumet.create_metric(
            "nvml_gpm_sm_util",
            Unit::Percent,
            "Percentage of time each multiprocessor had at least 1 warp assigned, averaged over all multiprocessors.",
        )?,

        // Percentage of warps that were active vs theoretical maximum, averaged over all multiprocessors.
        GpmMetricId::SmOccupancy => alumet.create_metric(
            "nvml_gpm_sm_occupancy",
            Unit::Percent,
            "Percentage of warps that were active vs theoretical maximum, averaged over all multiprocessors",
        )?,

        // Percentage of time the GPU's SMs were doing (any, DFMA, HMMA or IMMMA) tensor operations.
        GpmMetricId::AnyTensorUtil
        | GpmMetricId::DfmaTensorUtil
        | GpmMetricId::HmmaTensorUtil
        | GpmMetricId::ImmaTensorUtil => alumet.create_metric(
            "nvml_gpm_tensor_utilization",
            Unit::Percent,
            "Percentage of time the GPU's SMs were doing tensor operations.",
        )?,

        // Percentage of time the GPU's SMs were doing integer, FP64, FP32 or FP16 operations.
        GpmMetricId::IntegerUtil | GpmMetricId::Fp64Util | GpmMetricId::Fp32Util | GpmMetricId::Fp16Util => alumet
            .create_metric(
                "nvml_gpm_precision_utilization",
                Unit::Percent,
                "Percentage of time the GPU's SMs were doing operations in a specific precision.",
            )?,

        // Percentage of DRAM bandwidth used.
        GpmMetricId::DramBwUtil => alumet.create_metric(
            "nvml_gpm_dram_utilization",
            Unit::Percent,
            "Percentage of DRAM bandwidth used.",
        )?,

        // PCIe bytes transmitted or received per second.
        GpmMetricId::PcieTxPerSec | GpmMetricId::PcieRxPerSec => alumet.create_metric(
            "nvml_gpm_pcie_throughput",
            Unit::Byte,
            "PCIe bytes transmitted or received per second.",
        )?,

        // NVDEC instance (0..7) utilization.
        GpmMetricId::Nvdec0Util
        | GpmMetricId::Nvdec1Util
        | GpmMetricId::Nvdec2Util
        | GpmMetricId::Nvdec3Util
        | GpmMetricId::Nvdec4Util
        | GpmMetricId::Nvdec5Util
        | GpmMetricId::Nvdec6Util
        | GpmMetricId::Nvdec7Util => {
            alumet.create_metric("nvml_gpm_nvdec_utilization", Unit::Percent, "NVDEC utilization.")?
        }

        // NVJPG instance (0..7) utilization.
        GpmMetricId::Nvjpg0Util
        | GpmMetricId::Nvjpg1Util
        | GpmMetricId::Nvjpg2Util
        | GpmMetricId::Nvjpg3Util
        | GpmMetricId::Nvjpg4Util
        | GpmMetricId::Nvjpg5Util
        | GpmMetricId::Nvjpg6Util
        | GpmMetricId::Nvjpg7Util => {
            alumet.create_metric("nvml_gpm_nvjpg_utilization", Unit::Percent, "NVJPG utilization.")?
        }

        // NVOFA instance (0..1) utilization.
        GpmMetricId::Nvofa0Util | GpmMetricId::Nvofa1Util => {
            alumet.create_metric("nvml_gpm_nvofa_utilization", Unit::Percent, "NVOFA utilization.")?
        }

        // Total and instance (0..17) NVLink receive, transmit bytes per second across all links.
        GpmMetricId::NvlinkTotalRxPerSec
        | GpmMetricId::NvlinkTotalTxPerSec
        | GpmMetricId::NvlinkL0RxPerSec
        | GpmMetricId::NvlinkL0TxPerSec
        | GpmMetricId::NvlinkL1RxPerSec
        | GpmMetricId::NvlinkL1TxPerSec
        | GpmMetricId::NvlinkL2RxPerSec
        | GpmMetricId::NvlinkL2TxPerSec
        | GpmMetricId::NvlinkL3RxPerSec
        | GpmMetricId::NvlinkL3TxPerSec
        | GpmMetricId::NvlinkL4RxPerSec
        | GpmMetricId::NvlinkL4TxPerSec
        | GpmMetricId::NvlinkL5RxPerSec
        | GpmMetricId::NvlinkL5TxPerSec
        | GpmMetricId::NvlinkL6RxPerSec
        | GpmMetricId::NvlinkL6TxPerSec
        | GpmMetricId::NvlinkL7RxPerSec
        | GpmMetricId::NvlinkL7TxPerSec
        | GpmMetricId::NvlinkL8RxPerSec
        | GpmMetricId::NvlinkL8TxPerSec
        | GpmMetricId::NvlinkL9RxPerSec
        | GpmMetricId::NvlinkL9TxPerSec
        | GpmMetricId::NvlinkL10RxPerSec
        | GpmMetricId::NvlinkL10TxPerSec
        | GpmMetricId::NvlinkL11RxPerSec
        | GpmMetricId::NvlinkL11TxPerSec
        | GpmMetricId::NvlinkL12RxPerSec
        | GpmMetricId::NvlinkL12TxPerSec
        | GpmMetricId::NvlinkL13RxPerSec
        | GpmMetricId::NvlinkL13TxPerSec
        | GpmMetricId::NvlinkL14RxPerSec
        | GpmMetricId::NvlinkL14TxPerSec
        | GpmMetricId::NvlinkL15RxPerSec
        | GpmMetricId::NvlinkL15TxPerSec
        | GpmMetricId::NvlinkL16RxPerSec
        | GpmMetricId::NvlinkL16TxPerSec
        | GpmMetricId::NvlinkL17RxPerSec
        | GpmMetricId::NvlinkL17TxPerSec => alumet.create_metric(
            "nvml_gpm_nvlink_throughput",
            Unit::Byte,
            "NVLink bytes transmitted or received per second.",
        )?,
    })
}

/// Maps a [GpmMetricId] to a vector of attributes that should be added to the corresponding MeasurementPoint
pub fn match_gpm_id_to_attributes(gpm_metric: &GpmMetricId) -> Vec<(&'static str, AttributeValue)> {
    match gpm_metric {
        GpmMetricId::AnyTensorUtil => vec![("operation_type", AttributeValue::Str("any"))],
        GpmMetricId::DfmaTensorUtil => vec![("operation_type", AttributeValue::Str("dfma"))],
        GpmMetricId::HmmaTensorUtil => vec![("operation_type", AttributeValue::Str("hmma"))],
        GpmMetricId::ImmaTensorUtil => vec![("operation_type", AttributeValue::Str("imma"))],

        GpmMetricId::IntegerUtil => vec![("precision", AttributeValue::Str("integer"))],
        GpmMetricId::Fp64Util => vec![("precision", AttributeValue::Str("fp64"))],
        GpmMetricId::Fp32Util => vec![("precision", AttributeValue::Str("fp32"))],
        GpmMetricId::Fp16Util => vec![("precision", AttributeValue::Str("fp16"))],

        GpmMetricId::PcieTxPerSec => vec![("direction", AttributeValue::Str("transmitted"))],
        GpmMetricId::PcieRxPerSec => vec![("direction", AttributeValue::Str("received"))],

        GpmMetricId::Nvdec0Util => vec![("instance", AttributeValue::U64(0))],
        GpmMetricId::Nvdec1Util => vec![("instance", AttributeValue::U64(1))],
        GpmMetricId::Nvdec2Util => vec![("instance", AttributeValue::U64(2))],
        GpmMetricId::Nvdec3Util => vec![("instance", AttributeValue::U64(3))],
        GpmMetricId::Nvdec4Util => vec![("instance", AttributeValue::U64(4))],
        GpmMetricId::Nvdec5Util => vec![("instance", AttributeValue::U64(5))],
        GpmMetricId::Nvdec6Util => vec![("instance", AttributeValue::U64(6))],
        GpmMetricId::Nvdec7Util => vec![("instance", AttributeValue::U64(7))],

        GpmMetricId::Nvjpg0Util => vec![("instance", AttributeValue::U64(0))],
        GpmMetricId::Nvjpg1Util => vec![("instance", AttributeValue::U64(1))],
        GpmMetricId::Nvjpg2Util => vec![("instance", AttributeValue::U64(2))],
        GpmMetricId::Nvjpg3Util => vec![("instance", AttributeValue::U64(3))],
        GpmMetricId::Nvjpg4Util => vec![("instance", AttributeValue::U64(4))],
        GpmMetricId::Nvjpg5Util => vec![("instance", AttributeValue::U64(5))],
        GpmMetricId::Nvjpg6Util => vec![("instance", AttributeValue::U64(6))],
        GpmMetricId::Nvjpg7Util => vec![("instance", AttributeValue::U64(7))],

        GpmMetricId::Nvofa0Util => vec![("instance", AttributeValue::U64(0))],
        GpmMetricId::Nvofa1Util => vec![("instance", AttributeValue::U64(1))],

        GpmMetricId::NvlinkTotalRxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("total")),
        ],
        GpmMetricId::NvlinkTotalTxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("total")),
        ],

        GpmMetricId::NvlinkL0RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("0")),
        ],
        GpmMetricId::NvlinkL0TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("0")),
        ],
        GpmMetricId::NvlinkL1RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("1")),
        ],
        GpmMetricId::NvlinkL1TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("1")),
        ],
        GpmMetricId::NvlinkL2RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("2")),
        ],
        GpmMetricId::NvlinkL2TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("2")),
        ],
        GpmMetricId::NvlinkL3RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("3")),
        ],
        GpmMetricId::NvlinkL3TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("3")),
        ],
        GpmMetricId::NvlinkL4RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("4")),
        ],
        GpmMetricId::NvlinkL4TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("4")),
        ],
        GpmMetricId::NvlinkL5RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("5")),
        ],
        GpmMetricId::NvlinkL5TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("5")),
        ],
        GpmMetricId::NvlinkL6RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("6")),
        ],
        GpmMetricId::NvlinkL6TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("6")),
        ],
        GpmMetricId::NvlinkL7RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("7")),
        ],
        GpmMetricId::NvlinkL7TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("7")),
        ],
        GpmMetricId::NvlinkL8RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("8")),
        ],
        GpmMetricId::NvlinkL8TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("8")),
        ],
        GpmMetricId::NvlinkL9RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("9")),
        ],
        GpmMetricId::NvlinkL9TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("9")),
        ],
        GpmMetricId::NvlinkL10RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("10")),
        ],
        GpmMetricId::NvlinkL10TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("10")),
        ],
        GpmMetricId::NvlinkL11RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("11")),
        ],
        GpmMetricId::NvlinkL11TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("11")),
        ],
        GpmMetricId::NvlinkL12RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("12")),
        ],
        GpmMetricId::NvlinkL12TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("12")),
        ],
        GpmMetricId::NvlinkL13RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("13")),
        ],
        GpmMetricId::NvlinkL13TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("13")),
        ],
        GpmMetricId::NvlinkL14RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("14")),
        ],
        GpmMetricId::NvlinkL14TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("14")),
        ],
        GpmMetricId::NvlinkL15RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("15")),
        ],
        GpmMetricId::NvlinkL15TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("15")),
        ],
        GpmMetricId::NvlinkL16RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("16")),
        ],
        GpmMetricId::NvlinkL16TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("16")),
        ],
        GpmMetricId::NvlinkL17RxPerSec => vec![
            ("direction", AttributeValue::Str("received")),
            ("instance", AttributeValue::Str("17")),
        ],
        GpmMetricId::NvlinkL17TxPerSec => vec![
            ("direction", AttributeValue::Str("transmitted")),
            ("instance", AttributeValue::Str("17")),
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every GPM metric id known by nvml-wrapper.
    fn all_gpm_ids() -> Vec<GpmMetricId> {
        (0..=u8::MAX as u32)
            .filter_map(|i| GpmMetricId::try_from(i).ok())
            .collect()
    }

    mod match_gpm_id_to_attributes {
        use super::*;
        use pretty_assertions::assert_eq;

        /// Every GPM id is mapped to the expected attributes (the type of each value is shown).
        #[test]
        fn all_ids() {
            let rendered: String = all_gpm_ids()
                .iter()
                .map(|id| {
                    let attrs: Vec<_> = match_gpm_id_to_attributes(id)
                        .into_iter()
                        .map(|(k, v)| format!("{k}={v:?}"))
                        .collect();
                    let attrs = if attrs.is_empty() {
                        String::from("(none)")
                    } else {
                        attrs.join(", ")
                    };
                    format!("{id:?}: {attrs}\n")
                })
                .collect();
            insta::assert_snapshot!(rendered, @r#"
            GraphicsUtil: (none)
            SmUtil: (none)
            SmOccupancy: (none)
            IntegerUtil: precision=Str("integer")
            AnyTensorUtil: operation_type=Str("any")
            DfmaTensorUtil: operation_type=Str("dfma")
            HmmaTensorUtil: operation_type=Str("hmma")
            ImmaTensorUtil: operation_type=Str("imma")
            DramBwUtil: (none)
            Fp64Util: precision=Str("fp64")
            Fp32Util: precision=Str("fp32")
            Fp16Util: precision=Str("fp16")
            PcieTxPerSec: direction=Str("transmitted")
            PcieRxPerSec: direction=Str("received")
            Nvdec0Util: instance=U64(0)
            Nvdec1Util: instance=U64(1)
            Nvdec2Util: instance=U64(2)
            Nvdec3Util: instance=U64(3)
            Nvdec4Util: instance=U64(4)
            Nvdec5Util: instance=U64(5)
            Nvdec6Util: instance=U64(6)
            Nvdec7Util: instance=U64(7)
            Nvjpg0Util: instance=U64(0)
            Nvjpg1Util: instance=U64(1)
            Nvjpg2Util: instance=U64(2)
            Nvjpg3Util: instance=U64(3)
            Nvjpg4Util: instance=U64(4)
            Nvjpg5Util: instance=U64(5)
            Nvjpg6Util: instance=U64(6)
            Nvjpg7Util: instance=U64(7)
            Nvofa0Util: instance=U64(0)
            Nvofa1Util: instance=U64(1)
            NvlinkTotalRxPerSec: direction=Str("received"), instance=Str("total")
            NvlinkTotalTxPerSec: direction=Str("transmitted"), instance=Str("total")
            NvlinkL0RxPerSec: direction=Str("received"), instance=Str("0")
            NvlinkL0TxPerSec: direction=Str("transmitted"), instance=Str("0")
            NvlinkL1RxPerSec: direction=Str("received"), instance=Str("1")
            NvlinkL1TxPerSec: direction=Str("transmitted"), instance=Str("1")
            NvlinkL2RxPerSec: direction=Str("received"), instance=Str("2")
            NvlinkL2TxPerSec: direction=Str("transmitted"), instance=Str("2")
            NvlinkL3RxPerSec: direction=Str("received"), instance=Str("3")
            NvlinkL3TxPerSec: direction=Str("transmitted"), instance=Str("3")
            NvlinkL4RxPerSec: direction=Str("received"), instance=Str("4")
            NvlinkL4TxPerSec: direction=Str("transmitted"), instance=Str("4")
            NvlinkL5RxPerSec: direction=Str("received"), instance=Str("5")
            NvlinkL5TxPerSec: direction=Str("transmitted"), instance=Str("5")
            NvlinkL6RxPerSec: direction=Str("received"), instance=Str("6")
            NvlinkL6TxPerSec: direction=Str("transmitted"), instance=Str("6")
            NvlinkL7RxPerSec: direction=Str("received"), instance=Str("7")
            NvlinkL7TxPerSec: direction=Str("transmitted"), instance=Str("7")
            NvlinkL8RxPerSec: direction=Str("received"), instance=Str("8")
            NvlinkL8TxPerSec: direction=Str("transmitted"), instance=Str("8")
            NvlinkL9RxPerSec: direction=Str("received"), instance=Str("9")
            NvlinkL9TxPerSec: direction=Str("transmitted"), instance=Str("9")
            NvlinkL10RxPerSec: direction=Str("received"), instance=Str("10")
            NvlinkL10TxPerSec: direction=Str("transmitted"), instance=Str("10")
            NvlinkL11RxPerSec: direction=Str("received"), instance=Str("11")
            NvlinkL11TxPerSec: direction=Str("transmitted"), instance=Str("11")
            NvlinkL12RxPerSec: direction=Str("received"), instance=Str("12")
            NvlinkL12TxPerSec: direction=Str("transmitted"), instance=Str("12")
            NvlinkL13RxPerSec: direction=Str("received"), instance=Str("13")
            NvlinkL13TxPerSec: direction=Str("transmitted"), instance=Str("13")
            NvlinkL14RxPerSec: direction=Str("received"), instance=Str("14")
            NvlinkL14TxPerSec: direction=Str("transmitted"), instance=Str("14")
            NvlinkL15RxPerSec: direction=Str("received"), instance=Str("15")
            NvlinkL15TxPerSec: direction=Str("transmitted"), instance=Str("15")
            NvlinkL16RxPerSec: direction=Str("received"), instance=Str("16")
            NvlinkL16TxPerSec: direction=Str("transmitted"), instance=Str("16")
            NvlinkL17RxPerSec: direction=Str("received"), instance=Str("17")
            NvlinkL17TxPerSec: direction=Str("transmitted"), instance=Str("17")
            "#);
        }

        /// The ids of `all_gpm_ids` whose name starts with `prefix`, with their name.
        fn ids_named(prefix: &str) -> Vec<(GpmMetricId, String)> {
            all_gpm_ids()
                .into_iter()
                .map(|id| (id, format!("{id:?}")))
                .filter(|(_, name)| name.starts_with(prefix))
                .collect()
        }

        /// Each NVDEC, NVJPG and NVOFA instance is identified by the number in its id.
        #[test]
        fn engine_instance() {
            for (prefix, count) in [("Nvdec", 8), ("Nvjpg", 8), ("Nvofa", 2)] {
                let ids = ids_named(prefix);
                assert_eq!(ids.len(), count, "{prefix}");
                for (id, name) in ids {
                    let instance: u64 = name[prefix.len()..name.len() - "Util".len()].parse().unwrap();
                    let expected = vec![("instance", AttributeValue::U64(instance))];
                    assert_eq!(match_gpm_id_to_attributes(&id), expected, "{name}");
                }
            }
        }

        /// NVLink throughput is split by direction and by link, or "total" for all the links.
        #[test]
        fn nvlink_direction_and_instance() {
            let ids = ids_named("Nvlink");
            assert_eq!(ids.len(), 2 * 19, "18 links and the total, in both directions");
            for (id, name) in ids {
                // e.g. "NvlinkL13RxPerSec" or "NvlinkTotalTxPerSec"
                let link = &name["Nvlink".len()..name.len() - "RxPerSec".len()];
                let instance = match link {
                    "Total" => String::from("total"),
                    l => l.trim_start_matches('L').to_owned(),
                };
                let direction = if name.ends_with("RxPerSec") {
                    "received"
                } else {
                    "transmitted"
                };
                let actual: Vec<_> = match_gpm_id_to_attributes(&id)
                    .into_iter()
                    .map(|(k, v)| (k, v.to_string()))
                    .collect();
                let expected = vec![("direction", direction.to_owned()), ("instance", instance)];
                assert_eq!(actual, expected, "{name}");
            }
        }
    }
}
