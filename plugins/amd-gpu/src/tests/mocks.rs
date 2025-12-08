#[cfg(test)]
pub mod tests_mocks {
    use crate::{Config, bindings::*};

    pub const MOCK_TIMESTAMP: u64 = 1712024507665;
    pub const MOCK_UUID: &str = "a4ff740f-0000-1000-80ea-e05c945bb3b2";

    pub const MOCK_PROCESS_NAME: [i8; 256] = {
        let mut n = [0i8; 256];
        n[0] = b'p' as i8;
        n[1] = b'1' as i8;
        n
    };

    pub const MOCK_ENERGY: u64 = 123456789;
    pub const MOCK_ENERGY_RESOLUTION: f32 = 15.3;
    pub const MOCK_VOLTAGE: i64 = 850;

    pub const MOCK_ACTIVITY: amdsmi_engine_usage_t = amdsmi_engine_usage_t {
        gfx_activity: 131072,
        mm_activity: 262144,
        umc_activity: 524288,
        reserved: [0; 13],
    };

    pub const MOCK_PROCESS: amdsmi_proc_info_t = amdsmi_proc_info_t {
        name: MOCK_PROCESS_NAME,
        pid: 1,
        mem: 131072,
        engine_usage: amdsmi_proc_info_t_engine_usage_ {
            gfx: 1234567,
            enc: 2345678,
            reserved: [0; 12],
        },
        memory_usage: amdsmi_proc_info_t_memory_usage_ {
            gtt_mem: 1234567,
            cpu_mem: 2345678,
            vram_mem: 3456789,
            reserved: [0; 10],
        },
        container_name: MOCK_PROCESS_NAME,
        cu_occupancy: 123456789,
        evicted_time: 65535,
        reserved: [0; 10],
    };

    pub const MOCK_POWER: amdsmi_power_info_t = amdsmi_power_info_t {
        socket_power: 65535,
        current_socket_power: 45,
        average_socket_power: 43,
        gfx_voltage: 65535,
        soc_voltage: 65535,
        mem_voltage: 65535,
        power_limit: 65535,
        reserved: [0; 18],
    };

    pub const MOCK_TEMPERATURE: &[(amdsmi_temperature_type_t, i64)] = &[
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_EDGE, 45),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_JUNCTION, 46),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_0, 47),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_1, 48),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_2, 49),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_HBM_3, 50),
        (amdsmi_temperature_type_t_AMDSMI_TEMPERATURE_TYPE_PLX, 51),
    ];

    pub const MOCK_MEMORY: &[(amdsmi_memory_type_t, u64)] = &[
        (amdsmi_memory_type_t_AMDSMI_MEM_TYPE_VRAM, 131072),
        (amdsmi_memory_type_t_AMDSMI_MEM_TYPE_GTT, 262144),
    ];

    // Mock fake toml table for configuration
    pub fn config_to_toml_table(config: &Config) -> toml::Table {
        toml::Value::try_from(config).unwrap().as_table().unwrap().clone()
    }
}
