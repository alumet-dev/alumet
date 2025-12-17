use crate::bindings::{
    AMDSMI_GPU_UUID_SIZE, amdsmi_init_flags_t, amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS, amdsmi_memory_type_t,
    amdsmi_memory_type_t_AMDSMI_MEM_TYPE_GTT, amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, amdsmi_status_t,
    amdsmi_status_t_AMDSMI_STATUS_INVAL, amdsmi_status_t_AMDSMI_STATUS_NO_PERM,
    amdsmi_status_t_AMDSMI_STATUS_NOT_SUPPORTED, amdsmi_status_t_AMDSMI_STATUS_NOT_YET_IMPLEMENTED,
    amdsmi_status_t_AMDSMI_STATUS_OUT_OF_RESOURCES, amdsmi_status_t_AMDSMI_STATUS_SUCCESS,
    amdsmi_status_t_AMDSMI_STATUS_UNEXPECTED_DATA, amdsmi_status_t_AMDSMI_STATUS_UNKNOWN_ERROR,
    amdsmi_temperature_metric_t, amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT, amdsmi_temperature_type_t,
    amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE, amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_0,
    amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_1, amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_2,
    amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_3, amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HOTSPOT,
    amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_PLX, amdsmi_voltage_metric_t,
    amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT, amdsmi_voltage_type_t, amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX,
};

pub const LIB_PATH: &str = "libamd_smi.so";
pub const INIT_FLAG: amdsmi_init_flags_t = amdsmi_init_flags_t_AMDSMI_INIT_AMD_GPUS;
pub const UUID_LENGTH: u32 = AMDSMI_GPU_UUID_SIZE;

pub const SUCCESS: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_SUCCESS;
pub const ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_INVAL;
pub const OVERFLOW: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_OUT_OF_RESOURCES;

pub const METRIC_TEMP: amdsmi_temperature_metric_t = amdsmi_temperature_metric_t_AMDSMI_TEMP_CURRENT;
pub const VOLTAGE_SENSOR_TYPE: amdsmi_voltage_type_t = amdsmi_voltage_type_t_AMDSMI_VOLT_TYPE_VDDGFX;
pub const VOLTAGE_METRIC: amdsmi_voltage_metric_t = amdsmi_voltage_metric_t_AMDSMI_VOLT_CURRENT;

pub const NO_PERM: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NO_PERM;
pub const NOT_SUPPORTED: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NOT_SUPPORTED;
pub const NOT_YET_IMPLEMENTED: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_NOT_YET_IMPLEMENTED;
pub const UNEXPECTED_DATA: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_UNEXPECTED_DATA;
pub const UNKNOWN_ERROR: amdsmi_status_t = amdsmi_status_t_AMDSMI_STATUS_UNKNOWN_ERROR;

pub const PLUGIN_NAME: &str = "amd-gpu";

pub const METRIC_LABEL_ACTIVITY: &str = "amd_gpu_activity_usage";
pub const METRIC_LABEL_ENERGY: &str = "amd_gpu_energy_consumption";
pub const METRIC_LABEL_MEMORY: &str = "amd_gpu_memory_usage";
pub const METRIC_LABEL_POWER: &str = "amd_gpu_power_consumption";
pub const METRIC_LABEL_TEMPERATURE: &str = "amd_gpu_temperature";
pub const METRIC_LABEL_VOLTAGE: &str = "amd_gpu_voltage";
pub const METRIC_LABEL_PROCESS_MEMORY: &str = "amd_gpu_process_memory_usage";
pub const METRIC_LABEL_PROCESS_ENCODE: &str = "amd_gpu_process_engine_usage_encode";
pub const METRIC_LABEL_PROCESS_GFX: &str = "amd_gpu_process_engine_gfx";
pub const METRIC_LABEL_PROCESS_GTT: &str = "amd_gpu_process_memory_usage_gtt";
pub const METRIC_LABEL_PROCESS_CPU: &str = "amd_gpu_process_memory_usage_cpu";
pub const METRIC_LABEL_PROCESS_VRAM: &str = "amd_gpu_process_memory_usage_vram";

pub const MEMORY_TYPE: [(amdsmi_memory_type_t, &str); 2] = [
    (
        amdsmi_memory_type_t_AMDSMI_MEM_TYPE_GTT,
        "memory_graphic_translation_table",
    ),
    (amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, "memory_video_computing"),
];

pub const SENSOR_TYPE: [(amdsmi_temperature_type_t, &str); 7] = [
    (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE, "thermal_global"),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HOTSPOT,
        "thermal_hotspot",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_0,
        "thermal_high_bandwidth_memory_0",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_1,
        "thermal_high_bandwidth_memory_1",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_2,
        "thermal_high_bandwidth_memory_2",
    ),
    (
        amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_3,
        "thermal_high_bandwidth_memory_3",
    ),
    (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_PLX, "thermal_pci_bus"),
];
