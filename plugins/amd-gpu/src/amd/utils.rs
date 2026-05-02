use amd_smi_wrapper::metrics::{AmdMemoryType, AmdTemperatureType};

pub const PLUGIN_NAME: &str = "amd-gpu";

pub const METRIC_LABEL_ACTIVITY: &str = "amd_gpu_activity_usage";
pub const METRIC_LABEL_ENERGY: &str = "amd_gpu_energy_consumption";
pub const METRIC_LABEL_MEMORY: &str = "amd_gpu_memory_usage";
pub const METRIC_LABEL_POWER: &str = "amd_gpu_power_consumption";
pub const METRIC_LABEL_TEMPERATURE: &str = "amd_gpu_temperature";
pub const METRIC_LABEL_VOLTAGE: &str = "amd_gpu_voltage";
pub const METRIC_LABEL_PROCESS_COMPUTE_UNIT_OCCUPANCY: &str = "amd_gpu_process_compute_unit_occupancy";
pub const METRIC_LABEL_PROCESS_MEMORY: &str = "amd_gpu_process_memory_usage";
pub const METRIC_LABEL_PROCESS_ENCODE: &str = "amd_gpu_process_engine_usage_encode";
pub const METRIC_LABEL_PROCESS_GFX: &str = "amd_gpu_process_engine_gfx";
pub const METRIC_LABEL_PROCESS_GTT: &str = "amd_gpu_process_memory_usage_gtt";
pub const METRIC_LABEL_PROCESS_CPU: &str = "amd_gpu_process_memory_usage_cpu";
pub const METRIC_LABEL_PROCESS_VRAM: &str = "amd_gpu_process_memory_usage_vram";

pub const MEMORY_TYPE: [(AmdMemoryType, &str); 2] = [
    (AmdMemoryType::AMDSMI_MEM_TYPE_GTT, "memory_graphic_translation_table"),
    (AmdMemoryType::AMDSMI_MEM_TYPE_VRAM, "memory_video_computing"),
];

pub const SENSOR_TYPE: [(AmdTemperatureType, &str); 7] = [
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_EDGE, "thermal_global"),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HOTSPOT, "thermal_hotspot"),
    (
        AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_3,
        "thermal_high_bandwidth_memory_3",
    ),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_PLX, "thermal_pci_bus"),
];
