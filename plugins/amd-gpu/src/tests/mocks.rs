use amd_smi_wrapper::utils::{
    AmdEnergyConsumption, AmdEngineUsage, AmdMemoryType, AmdPowerConsumption, AmdProcess, AmdProcessEngineUsage,
    AmdProcessMemoryUsage, AmdTemperatureType,
};

pub const MOCK_SOURCE_NAME: &str = "amd_gpu_devices";
pub const MOCK_TIMESTAMP: u64 = 1712024507665;
pub const MOCK_UUID: &str = "a4ff740f-0000-1000-80ea-e05c945bb3b2";
pub const MOCK_PROCESS_NAME: &str = "p1";

pub const MOCK_VOLTAGE: i64 = 850;

pub const MOCK_ACTIVITY: AmdEngineUsage = AmdEngineUsage {
    gfx_activity: 131072,
    mm_activity: 262144,
    umc_activity: 524288,
};

pub const MOCK_ENERGY: AmdEnergyConsumption = AmdEnergyConsumption {
    energy: 123456789,
    resolution: 15.3,
    timestamp: MOCK_TIMESTAMP,
};

pub const MOCK_POWER: AmdPowerConsumption = AmdPowerConsumption {
    socket_power: 43,
    current_socket_power: 45,
    average_socket_power: 47,
    gfx_voltage: 65535,
    soc_voltage: 65535,
    mem_voltage: 65535,
    power_limit: 65535,
};

pub const MOCK_TEMPERATURE: &[(AmdTemperatureType, i64)] = &[
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_EDGE, 45),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HOTSPOT, 46),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_0, 47),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_1, 48),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_2, 49),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_HBM_3, 50),
    (AmdTemperatureType::AMDSMI_TEMPERATURE_TYPE_PLX, 51),
];

pub const MOCK_MEMORY: &[(AmdMemoryType, u64)] = &[
    (AmdMemoryType::AMDSMI_MEM_TYPE_VRAM, 131072),
    (AmdMemoryType::AMDSMI_MEM_TYPE_GTT, 262144),
];

pub fn mock_process() -> AmdProcess {
    AmdProcess {
        name: MOCK_PROCESS_NAME.to_string(),
        pid: 1,
        mem: 131072,
        engine_usage: AmdProcessEngineUsage {
            gfx: 1234567,
            enc: 2345678,
        },
        memory_usage: AmdProcessMemoryUsage {
            gtt_mem: 1234567,
            cpu_mem: 2345678,
            vram_mem: 3456789,
        },
        container_name: MOCK_PROCESS_NAME.to_string(),
        cu_occupancy: 42,
        evicted_time: 65535,
    }
}
