use alumet::{
    metrics::{TypedMetricId, error::MetricCreationError},
    plugin::AlumetPluginStart,
    units::{PrefixedUnit, Unit},
};

use crate::amd::utils::{
    METRIC_LABEL_ACTIVITY, METRIC_LABEL_ENERGY, METRIC_LABEL_MEMORY, METRIC_LABEL_POWER, METRIC_LABEL_PROCESS_CPU,
    METRIC_LABEL_PROCESS_ENCODE, METRIC_LABEL_PROCESS_GFX, METRIC_LABEL_PROCESS_GTT, METRIC_LABEL_PROCESS_MEMORY,
    METRIC_LABEL_PROCESS_VRAM, METRIC_LABEL_TEMPERATURE, METRIC_LABEL_VOLTAGE,
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
                METRIC_LABEL_ACTIVITY,
                Unit::Percent,
                "Get GPU activity usage in percentage",
            )?,
            gpu_energy_consumption: alumet.create_metric::<f64>(
                METRIC_LABEL_ENERGY,
                PrefixedUnit::milli(Unit::Joule),
                "Get GPU energy consumption in millijoule",
            )?,
            gpu_memory_usages: alumet.create_metric::<u64>(
                METRIC_LABEL_MEMORY,
                Unit::Byte,
                "Get GPU used memory in byte",
            )?,
            gpu_power_consumption: alumet.create_metric::<u64>(
                METRIC_LABEL_POWER,
                Unit::Watt,
                "Get GPU electric average power consumption in watts",
            )?,
            gpu_temperatures: alumet.create_metric::<u64>(
                METRIC_LABEL_TEMPERATURE,
                Unit::DegreeCelsius,
                "Get GPU temperature in °C",
            )?,
            gpu_voltage: alumet.create_metric::<u64>(
                METRIC_LABEL_VOLTAGE,
                PrefixedUnit::milli(Unit::Volt),
                "Get GPU voltage in millivolt",
            )?,
            process_memory_usage: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_MEMORY,
                Unit::Byte,
                "Get process memory usage in byte",
            )?,
            process_engine_usage_encode: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_ENCODE,
                PrefixedUnit::nano(Unit::Second),
                "Get process encode engine usage in nanosecond",
            )?,
            process_engine_usage_gfx: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_GFX,
                PrefixedUnit::nano(Unit::Second),
                "Get process GFX engine usage in nanosecond",
            )?,
            process_memory_usage_gtt: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_GTT,
                Unit::Byte,
                "Get process GTT memory usage in byte",
            )?,
            process_memory_usage_cpu: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_CPU,
                Unit::Byte,
                "Get process CPU memory usage in byte",
            )?,
            process_memory_usage_vram: alumet.create_metric::<u64>(
                METRIC_LABEL_PROCESS_VRAM,
                Unit::Byte,
                "Get process VRAM memory usage in byte",
            )?,
        })
    }
}

#[cfg(test)]
mod tests_metrics {
    use super::*;
    use crate::{
        AmdError, AmdGpuPlugin, Config,
        amd::utils::{METRIC_TEMP, PLUGIN_NAME, UNEXPECTED_DATA},
        interface::{AmdSmiRef, MockProcessorProvider, MockSocketHandle},
        tests::mocks::tests_mocks::{
            MOCK_ACTIVITY, MOCK_ENERGY, MOCK_ENERGY_RESOLUTION, MOCK_MEMORY, MOCK_POWER, MOCK_TEMPERATURE,
            MOCK_TIMESTAMP, MOCK_UUID, MOCK_VOLTAGE, config_to_toml_table,
        },
    };
    use alumet::{
        agent::{
            self,
            plugin::{PluginInfo, PluginSet},
        },
        plugin::PluginMetadata,
        test::StartupExpectations,
    };
    use std::time::Duration;

    // Test `start` function for amd-gpu plugin metric collect without device
    #[test]
    fn test_start_error() {
        let mut mock_init = AmdSmiRef::new();
        let mut mock_socket = MockSocketHandle::new();
        let mut mock_processor = MockProcessorProvider::new();

        mock_processor
            .expect_get_device_uuid()
            .returning(|| Ok(MOCK_UUID.to_owned()));

        mock_processor
            .expect_get_device_activity()
            .returning(|| Ok(MOCK_ACTIVITY));

        mock_processor
            .expect_get_device_energy_consumption()
            .returning(|| Ok((MOCK_ENERGY, MOCK_ENERGY_RESOLUTION, MOCK_TIMESTAMP)));

        mock_processor
            .expect_get_device_power_consumption()
            .returning(|| Ok(MOCK_POWER));

        mock_processor
            .expect_get_device_power_managment()
            .returning(|| Ok(true));
        mock_processor.expect_get_device_process_list().returning(|| Ok(vec![]));
        mock_processor
            .expect_get_device_voltage()
            .returning(|_, _| Ok(MOCK_VOLTAGE));

        mock_processor.expect_get_device_memory_usage().returning(|mem_type| {
            MOCK_MEMORY
                .iter()
                .find(|(t, _)| *t == mem_type)
                .map(|(_, v)| Ok(*v))
                .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
        });

        mock_processor
            .expect_get_device_temperature()
            .returning(|sensor, metric| {
                if metric != METRIC_TEMP {
                    return Err(AmdError(UNEXPECTED_DATA));
                }
                MOCK_TEMPERATURE
                    .iter()
                    .find(|(s, _)| *s == sensor)
                    .map(|(_, v)| Ok(*v))
                    .unwrap_or(Err(AmdError(UNEXPECTED_DATA)))
            });

        mock_socket
            .expect_get_processor_handles()
            .return_once(move || Ok(vec![mock_processor]));

        mock_init
            .expect_get_socket_handles()
            .return_once(move || Ok(vec![mock_socket]));

        let mut plugins = PluginSet::new();
        let config = Config { ..Default::default() };

        plugins.add_plugin(PluginInfo {
            metadata: PluginMetadata::from_static::<AmdGpuPlugin>(),
            enabled: true,
            config: Some(config_to_toml_table(&config)),
        });

        let startup_expectation = StartupExpectations::new()
            .expect_metric::<f64>(METRIC_LABEL_ACTIVITY, Unit::Percent.clone())
            .expect_metric::<f64>(METRIC_LABEL_ENERGY, PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>(METRIC_LABEL_MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_POWER, Unit::Watt.clone())
            .expect_metric::<u64>(METRIC_LABEL_TEMPERATURE, Unit::DegreeCelsius.clone())
            .expect_metric::<u64>(METRIC_LABEL_VOLTAGE, PrefixedUnit::milli(Unit::Volt))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_MEMORY, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_ENCODE, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_GFX, PrefixedUnit::nano(Unit::Second))
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_GTT, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_CPU, Unit::Byte.clone())
            .expect_metric::<u64>(METRIC_LABEL_PROCESS_VRAM, Unit::Byte.clone())
            .expect_source(PLUGIN_NAME, MOCK_UUID);

        let agent = agent::Builder::new(plugins)
            .with_expectations(startup_expectation)
            .build_and_start()
            .unwrap();

        // Send shutdown message
        agent.wait_for_shutdown(Duration::from_secs(5)).unwrap();
    }
}
