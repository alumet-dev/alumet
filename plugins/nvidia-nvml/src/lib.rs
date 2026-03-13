use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use alumet::{
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{
        ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};

use crate::{
    metrics::{FullMetrics, MinimalMetrics},
    nvml::{
        NvmlDevice, NvmlProvider,
        detect::{DeviceFailureStrategy, Devices},
    },
    probe::SourceProvider,
};

mod metrics;
mod nvml;
mod probe;

pub use nvml::objects::NvmlLoader;

pub struct NvmlPlugin<P: NvmlProvider + 'static> {
    config: Config,
    nvml: P::Lib,
}

impl<P: NvmlProvider + 'static> AlumetPlugin for NvmlPlugin<P> {
    #[cfg(not(tarpaulin_include))]
    fn name() -> &'static str {
        "nvml"
    }

    #[cfg(not(tarpaulin_include))]
    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    #[cfg(not(tarpaulin_include))]
    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        let nvml = P::get().context("failed to init the nvml library")?;
        Ok(Box::new(NvmlPlugin { config, nvml }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let failure_strategy = match self.config.skip_failed_devices {
            true => DeviceFailureStrategy::Skip,
            false => DeviceFailureStrategy::Fail,
        };
        let detected = Devices::detect(&self.nvml, failure_strategy)?;
        let stats = detected.detection_stats();
        if stats.found_devices == 0 {
            return Err(anyhow!(
                "No NVML-compatible GPU found. If your device is a Jetson edge device, please disable the `nvml` feature of the plugin."
            ));
        }
        if stats.working_devices == 0 {
            return Err(anyhow!(
                "{} NVML-compatible devices found but none of them is working (see previous warnings).",
                stats.found_devices
            ));
        }

        for device in detected.iter() {
            let device_bus_id = device.inner.bus_id();
            let device_name = device.inner.name();
            log::info!(
                "Found NVML device {} \"{}\" with features: {}",
                device_bus_id,
                device_name,
                device.features
            );
        }
        let source_provider = match self.config.mode {
            Mode::Full => SourceProvider::Full(FullMetrics::new(alumet)?),
            Mode::Minimal => SourceProvider::Minimal(MinimalMetrics::new(alumet)?),
        };

        for device in detected.into_iter() {
            let source_name = format!("device_{}", device.inner.bus_id());
            let trigger = TriggerSpec::builder(self.config.poll_interval)
                .flush_interval(self.config.flush_interval)
                .build()?;

            match &source_provider {
                SourceProvider::Full(metrics) => {
                    // In full mode, the measurement operation can take a long time (> 10ms).
                    // To avoid slowing down the rest of the pipeline, we tell Alumet that the source is blocking, so that it is isolated.
                    let source = probe::FullSource::new(device, metrics.clone())?;
                    alumet.add_blocking_source(&source_name, Box::new(source), trigger)?;
                }
                SourceProvider::Minimal(metrics) => {
                    let source = probe::MinimalSource::new(device, metrics.clone())?;
                    alumet.add_source(&source_name, Box::new(source), trigger)?;
                }
            };
        }
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Initial interval between two Nvidia measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,

    /// Initial interval between two flushing of Nvidia measurements.
    #[serde(with = "humantime_serde")]
    flush_interval: Duration,

    /// On startup, the plugin inspects the GPU devices and detect their features.
    /// If `skip_failed_devices = true`, inspection failures will be logged and the plugin will continue.
    /// If `skip_failed_devices = true`, the first failure will make the plugin's startup fail.
    #[serde(default = "default_true")]
    skip_failed_devices: bool,

    /// In "full" mode, get many measurements from the GPU on each poll.
    /// In "minimal" mode, only measure the power consumption (it must be supported by the GPU).
    ///
    /// On some GPUs, the "full" mode is too slow for high frequencies (100 Hz can be hard to reach in full mode).
    mode: Mode,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    /// Gathers many NVML metrics.
    Full,
    /// Only measure the power consumption, and estimate the energy from the power.
    Minimal,
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            skip_failed_devices: true,
            mode: Mode::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    use alumet::{
        agent::plugin::{PluginInfo, PluginSet},
        measurement::MeasurementPoint,
        pipeline::naming::SourceName,
        plugin::PluginMetadata,
        resources::ResourceConsumer,
        test::{RuntimeExpectations, StartupExpectations, runtime::SourceCheckOutputContext},
        units::{PrefixedUnit, Unit},
    };
    use enum_map::{Enum, EnumMap};
    use nvml_wrapper::{
        enums::device::UsedGpuMemory,
        struct_wrappers::device::{ProcessInfo, ProcessUtilizationSample, Utilization},
        structs::device::UtilizationInfo,
    };

    use crate::nvml::{MockNvmlDevice, MockNvmlLib};

    use super::*;

    struct MockProvider;

    impl NvmlProvider for MockProvider {
        type Lib = MockNvmlLib;

        fn get() -> anyhow::Result<Self::Lib> {
            panic!("the plugin should be initialized with a custom metadata")
        }
    }

    impl NvmlPlugin<MockProvider> {
        pub fn test_metadata(nvml: MockNvmlLib) -> PluginMetadata {
            PluginMetadata {
                name: String::from("nvml"),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                init: Box::new(move |config| {
                    let config = deserialize_config(config)?;
                    Ok(Box::new(Self { config, nvml }))
                }),
                default_config: Box::new(Self::default_config),
            }
        }
    }

    const MOCK_NAME: &str = "Mock GPU";
    const MOCK_BUS_ID: &str = "00001999:77:00.0";

    #[derive(Default)]
    struct MockDeviceState {
        n_calls: EnumMap<DeviceMethod, u8>,
    }

    impl MockDeviceState {
        fn reset(&mut self) {
            self.n_calls = Default::default();
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug, Enum)]
    enum DeviceMethod {
        TotalEnergyConsumption,
        PowerUsage,
        ProcessUtilizationStats,
        // other values could be added in the future
    }

    fn mock_no_gpu(mock: &mut MockNvmlLib) {
        mock.expect_device_count().returning(|| Ok(0)).times(1);
        mock.expect_device_by_index().never();
    }

    fn mock_one_gpu(mock: &mut MockNvmlLib) -> Arc<Mutex<MockDeviceState>> {
        let state = Arc::new(Mutex::new(MockDeviceState::default()));
        let ret = Arc::clone(&state);
        mock.expect_device_count().returning(|| Ok(1));
        mock.expect_device_by_index().returning(move |i| {
            assert_eq!(i, 0, "invalid index");
            let mut device = MockNvmlDevice::new();
            device.expect_name().return_const(MOCK_NAME.to_owned());
            device.expect_bus_id().return_const(MOCK_BUS_ID.to_owned());
            device.expect_total_energy_consumption().returning({
                // change the energy counter on each call
                let state = Arc::clone(&state);
                move || {
                    let n = &mut state.lock().unwrap().n_calls[DeviceMethod::TotalEnergyConsumption];
                    let res = match n {
                        0 => Ok(10),
                        1 => Ok(5000),
                        _ => panic!("total_energy_consumption() called too many times"),
                    };
                    *n += 1;
                    res
                }
            });
            device.expect_power_usage().returning({
                // change the power value on each call
                let state = Arc::clone(&state);
                move || {
                    let n = &mut state.lock().unwrap().n_calls[DeviceMethod::PowerUsage];
                    let res = match n {
                        0 => Ok(100),
                        1 => Ok(150),
                        _ => panic!("power_usage() called too many times"),
                    };
                    *n += 1;
                    res
                }
            });
            device.expect_temperature().returning(|_| Ok(65));
            device
                .expect_utilization_rates()
                .returning(|| Ok(Utilization { gpu: 50, memory: 60 }));
            device.expect_decoder_utilization().returning(|| {
                Ok(UtilizationInfo {
                    utilization: 50,
                    sampling_period: 1_000, // 1 ms
                })
            });
            device.expect_encoder_utilization().returning(|| {
                Ok(UtilizationInfo {
                    utilization: 1,
                    sampling_period: 10_000, // 10 ms
                })
            });
            device.expect_process_utilization_stats().returning({
                let state = Arc::clone(&state);
                move |_timestamp| {
                    let n = &mut state.lock().unwrap().n_calls[DeviceMethod::ProcessUtilizationStats];
                    let res = match n {
                        0 => Ok(vec![ProcessUtilizationSample {
                            pid: 1234,
                            timestamp: 200_000,
                            sm_util: 50,
                            mem_util: 60,
                            enc_util: 1,
                            dec_util: 50,
                        }]),
                        _ => panic!("process_utilization_stats() called too many times"),
                    };
                    *n += 1;
                    res
                }
            });
            device.expect_running_compute_processes_count().returning(|| Ok(0));
            device.expect_running_compute_processes().returning(|| Ok(Vec::new()));
            device.expect_running_graphics_processes_count().returning(|| Ok(1));
            device.expect_running_graphics_processes().returning(|| {
                Ok(vec![ProcessInfo {
                    pid: 1234,
                    used_gpu_memory: UsedGpuMemory::Used(64_000),
                    gpu_instance_id: None,
                    compute_instance_id: None,
                }])
            });
            Ok(device)
        });
        ret
    }

    #[test]
    fn fail_plugin_no_gpu() {
        let mut nvml = MockNvmlLib::new();
        mock_no_gpu(&mut nvml);

        let config = serialize_config(super::Config::default()).unwrap();
        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: NvmlPlugin::test_metadata(nvml),
            enabled: true,
            config: Some(config.0),
        });
        let agent = alumet::agent::Builder::new(plugins);
        let running = agent.build_and_start();
        assert!(running.is_err(), "should fail because there is no gpu");
    }

    #[test]
    fn run_plugin_minimal() {
        let mut nvml = MockNvmlLib::new();
        let device_state = mock_one_gpu(&mut nvml);

        let config = serialize_config(super::Config {
            mode: Mode::Minimal,
            ..Default::default()
        })
        .unwrap();

        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: NvmlPlugin::test_metadata(nvml),
            enabled: true,
            config: Some(config.0),
        });
        let agent = alumet::agent::Builder::new(plugins);

        let startup_checks = StartupExpectations::new()
            .expect_metric::<f64>("nvml_energy_consumption", PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>("nvml_instant_power", PrefixedUnit::milli(Unit::Watt));

        let source_name = SourceName::from_str("nvml", &format!("device_{MOCK_BUS_ID}"));
        let runtime_checks = RuntimeExpectations::new()
            .test_source(
                source_name.clone(),
                move || {
                    // The mock has already been configured.
                    // Reset the counters, so that the features detection does not count in later calls.
                    device_state.lock().unwrap().reset();
                },
                |out| {
                    // first trigger, the source only produces the instant power
                    let points = out.measurements().iter().collect::<Vec<_>>();
                    assert_eq!(points.len(), 1, "wrong number of points, got {points:?}");
                    assert_eq!(points[0].value.as_u64(), 100);
                },
            )
            .test_source(
                source_name,
                || {
                    // The mock will be called a second time, and will change some of the values it returns.
                },
                |out| {
                    // second trigger, the source returns the instant power *and* the energy, computed from the 2 power values.
                    // previous power: 100
                    // new power: 150
                    // energy: ?
                    let points = out.points_by_metric_and_consumer();
                    assert_eq!(points.len(), 2, "wrong number of points, got {points:?}");
                    assert_eq!(
                        points[&("nvml_instant_power", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        150
                    );

                    // FIXME: the value depends on the time difference between the two calls, and we cannot mock that for the moment…
                    assert!(
                        points[&("nvml_energy_consumption", ResourceConsumer::LocalMachine)]
                            .value
                            .as_f64()
                            >= 0.0
                    );
                },
            );

        let running = agent
            .with_expectations(startup_checks)
            .with_expectations(runtime_checks)
            .build_and_start()
            .expect("should start fine");
        running
            .wait_for_shutdown(Duration::from_secs(1))
            .expect("error(s) detected");
    }

    #[test]
    fn run_plugin_full() {
        let mut nvml = MockNvmlLib::new();
        let device_state = mock_one_gpu(&mut nvml);

        let config = serialize_config(super::Config {
            mode: Mode::Full,
            ..Default::default()
        })
        .unwrap();

        let mut plugins = PluginSet::new();
        plugins.add_plugin(PluginInfo {
            metadata: NvmlPlugin::test_metadata(nvml),
            enabled: true,
            config: Some(config.0),
        });
        let agent = alumet::agent::Builder::new(plugins);

        let startup_checks = StartupExpectations::new()
            .expect_metric::<f64>("nvml_energy_consumption", PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>("nvml_instant_power", PrefixedUnit::milli(Unit::Watt))
            .expect_metric::<u64>("nvml_temperature_gpu", Unit::DegreeCelsius)
            .expect_metric::<u64>("nvml_gpu_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_decoder_sampling_period", PrefixedUnit::micro(Unit::Second))
            .expect_metric::<u64>("nvml_encoder_sampling_period", PrefixedUnit::micro(Unit::Second))
            .expect_metric::<u64>("nvml_n_compute_processes", Unit::Unity)
            .expect_metric::<u64>("nvml_n_graphic_processes", Unit::Unity)
            .expect_metric::<u64>("nvml_memory_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_decoder_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_encoder_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_sm_utilization", Unit::Percent);

        let source_name = SourceName::from_str("nvml", &format!("device_{MOCK_BUS_ID}"));
        let runtime_checks = RuntimeExpectations::new()
            .test_source(
                source_name.clone(),
                move || {
                    // The mock has already been configured.
                    // Reset the counters, so that the features detection does not count in later calls.
                    device_state.lock().unwrap().reset();
                },
                |out| {
                    // first trigger
                    let points = out.points_by_metric_and_consumer();

                    assert_eq!(points.len(), 10, "wrong number of points, got {points:?}");

                    assert_eq!(
                        points[&("nvml_encoder_utilization", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        1
                    );
                    assert_eq!(
                        points[&("nvml_decoder_utilization", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        50
                    );
                    assert_eq!(
                        points[&("nvml_gpu_utilization", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        50
                    );
                    assert_eq!(
                        points[&("nvml_memory_utilization", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        60
                    );
                    assert_eq!(
                        points[&("nvml_n_compute_processes", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        0
                    );
                    assert_eq!(
                        points[&("nvml_n_graphic_processes", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        1
                    );
                    assert_eq!(
                        points[&("nvml_temperature_gpu", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        65
                    );
                    assert_eq!(
                        points[&("nvml_instant_power", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        100
                    );
                    assert_eq!(
                        points[&("nvml_encoder_sampling_period", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        10_000
                    );
                    assert_eq!(
                        points[&("nvml_decoder_sampling_period", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        1_000
                    );
                },
            )
            .test_source(
                source_name,
                || {},
                |out| {
                    // second trigger
                    let points = out.points_by_metric_and_consumer();
                    assert_eq!(points.len(), 15, "wrong number of points, got {points:?}");

                    // new power value
                    assert_eq!(
                        points[&("nvml_instant_power", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        150
                    );

                    // previous energy counter: 10
                    // new energy counter: 5000
                    // expected difference : 4990
                    assert_eq!(
                        points[&("nvml_energy_consumption", ResourceConsumer::LocalMachine)]
                            .value
                            .as_u64(),
                        4990
                    );

                    // per-process metrics
                    assert_eq!(
                        points[&("nvml_sm_utilization", ResourceConsumer::Process { pid: 1234 })]
                            .value
                            .as_u64(),
                        50
                    );
                },
            );

        let running = agent
            .with_expectations(startup_checks)
            .with_expectations(runtime_checks)
            .build_and_start()
            .expect("should start fine");
        running
            .wait_for_shutdown(Duration::from_secs(1))
            .expect("error(s) detected");
    }
}
