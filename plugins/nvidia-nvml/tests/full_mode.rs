//! Integration tests of the full source, with a mock GPU that supports every feature except GPM.

use std::time::Duration;

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint},
    metrics::registry::MetricRegistry,
    pipeline::naming::SourceName,
    plugin::{PluginMetadata, rust::AlumetPlugin},
    resources::{Resource, ResourceConsumer},
    test::{RuntimeExpectations, StartupExpectations},
};
use nvml_wrapper::{
    enum_wrappers::device::Clock,
    enums::device::UsedGpuMemory,
    struct_wrappers::device::{MemoryInfo, ProcessInfo, ProcessUtilizationSample, Utilization},
    structs::device::UtilizationInfo,
};
use plugin_nvidia_nvml::{
    NvmlPlugin,
    mock::{MockNvmlDevice, MockNvmlLib, NvmlProvider},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const BUS_ID: &str = "00000000:01:00.0"; // Id of false GPU
const ENERGY_STEP: u64 = 5_000; // Energy consumed between two calls of `total_energy_consumption`, in mJ.
const COMPUTE_PID: u32 = 1234; // compute process of mock GPU
const GRAPHICS_PID: u32 = 5678; // graphic process of mock GPU

// Values returned by the mock GPU, and expected in the measurements.
// They are all different, so that a metric that reads the wrong value is detected.
const POWER_MW: u32 = 190_000;
const POWER_LIMIT_MW: u32 = 250_000;
const TEMPERATURE_C: u32 = 57;
const GPU_UTILIZATION: u32 = 40; // %
const MEMORY_UTILIZATION: u32 = 30; // %
const MEMORY_FREE_BYTES: u64 = 1_000;
const MEMORY_RESERVED_BYTES: u64 = 200;
const MEMORY_TOTAL_BYTES: u64 = 4_000;
const MEMORY_USED_BYTES: u64 = 2_800;
const DECODER_UTILIZATION: u32 = 15; // %
const DECODER_SAMPLING_US: u32 = 1_100;
const ENCODER_UTILIZATION: u32 = 25; // %
const ENCODER_SAMPLING_US: u32 = 2_000;
const CLOCK_SM_MHZ: u32 = 1;
const CLOCK_VIDEO_MHZ: u32 = 2;
const CLOCK_MEMORY_MHZ: u32 = 3;
const CLOCK_GRAPHICS_MHZ: u32 = 4;

// Processes running on the mock GPU.
const N_COMPUTE_PROCESSES: u32 = 1;
const N_GRAPHICS_PROCESSES: u32 = 2;
const COMPUTE_GPU_MEMORY_BYTES: u64 = 64_000;
const GRAPHICS_GPU_MEMORY_BYTES: u64 = 32_000;
const GRAPHICS_PARTITION_ID: u32 = 0; // MIG partition of the graphics process
const GRAPHICS_COMPUTE_INSTANCE_ID: u32 = 1;
const PROCESS_SM_UTILIZATION: u32 = 50; // %
const PROCESS_MEMORY_UTILIZATION: u32 = 60; // %
const PROCESS_ENCODER_UTILIZATION: u32 = 5; // %
const PROCESS_DECODER_UTILIZATION: u32 = 7; // %

/// A GPU that supports every feature except GPM.
fn device() -> MockNvmlDevice {
    let mut device = MockNvmlDevice::new();
    device.expect_name().return_const(String::from("Mock Gigaflop GPU"));
    device.expect_bus_id().return_const(String::from(BUS_ID));

    // the energy counter increases by ENERGY_STEP at each call
    let mut energy = 1_000_000;
    device.expect_total_energy_consumption().returning(move || {
        energy += ENERGY_STEP;
        Ok(energy)
    });
    device.expect_power_usage().returning(|| Ok(POWER_MW));
    device.expect_temperature().returning(|_| Ok(TEMPERATURE_C));
    device.expect_utilization_rates().returning(|| {
        Ok(Utilization {
            gpu: GPU_UTILIZATION,
            memory: MEMORY_UTILIZATION,
        })
    });
    device.expect_memory_info().returning(|| {
        Ok(MemoryInfo {
            free: MEMORY_FREE_BYTES,
            reserved: MEMORY_RESERVED_BYTES,
            total: MEMORY_TOTAL_BYTES,
            used: MEMORY_USED_BYTES,
            version: 2,
        })
    });
    device.expect_decoder_utilization().returning(|| {
        Ok(UtilizationInfo {
            utilization: DECODER_UTILIZATION,
            sampling_period: DECODER_SAMPLING_US,
        })
    });
    device.expect_encoder_utilization().returning(|| {
        Ok(UtilizationInfo {
            utilization: ENCODER_UTILIZATION,
            sampling_period: ENCODER_SAMPLING_US,
        })
    });
    device.expect_process_utilization_stats().returning(|_| {
        Ok(vec![ProcessUtilizationSample {
            pid: COMPUTE_PID,
            timestamp: 0,
            sm_util: PROCESS_SM_UTILIZATION,
            mem_util: PROCESS_MEMORY_UTILIZATION,
            enc_util: PROCESS_ENCODER_UTILIZATION,
            dec_util: PROCESS_DECODER_UTILIZATION,
        }])
    });

    // one compute process on the whole GPU, one graphics process on a MIG partition
    device
        .expect_running_compute_processes_count()
        .returning(|| Ok(N_COMPUTE_PROCESSES));
    device.expect_running_compute_processes().returning(|| {
        Ok(vec![ProcessInfo {
            pid: COMPUTE_PID,
            used_gpu_memory: UsedGpuMemory::Used(COMPUTE_GPU_MEMORY_BYTES),
            gpu_instance_id: None,
            compute_instance_id: None,
        }])
    });
    device
        .expect_running_graphics_processes_count()
        .returning(|| Ok(N_GRAPHICS_PROCESSES));
    device.expect_running_graphics_processes().returning(|| {
        Ok(vec![ProcessInfo {
            pid: GRAPHICS_PID,
            used_gpu_memory: UsedGpuMemory::Used(GRAPHICS_GPU_MEMORY_BYTES),
            gpu_instance_id: Some(GRAPHICS_PARTITION_ID),
            compute_instance_id: Some(GRAPHICS_COMPUTE_INSTANCE_ID),
        }])
    });

    device.expect_clock_info().returning(|clock| match clock {
        Clock::SM => Ok(CLOCK_SM_MHZ),
        Clock::Video => Ok(CLOCK_VIDEO_MHZ),
        Clock::Memory => Ok(CLOCK_MEMORY_MHZ),
        Clock::Graphics => Ok(CLOCK_GRAPHICS_MHZ),
    });
    device.expect_power_management_limit().returning(|| Ok(POWER_LIMIT_MW));
    device.expect_gpm_support().returning(|| Ok(false));
    device
}

/// Provides a NVML library with one GPU, which supports every feature except GPM.
struct GigaFlop;

impl NvmlProvider for GigaFlop {
    type Lib = MockNvmlLib;

    fn get() -> anyhow::Result<MockNvmlLib> {
        let mut lib = MockNvmlLib::new();
        lib.expect_device_count().returning(|| Ok(1));
        lib.expect_device_by_index().returning(|_| Ok(device()));
        Ok(lib)
    }
}

type Plugin = NvmlPlugin<GigaFlop>;

fn default_config() -> toml::Table {
    Plugin::default_config().unwrap().unwrap().0
}

fn plugin_set(config: toml::Table) -> PluginSet {
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<Plugin>(),
        enabled: true,
        config: Some(config),
    });
    plugins
}

/// Starts the plugin, polls the GPU source `n_polls` times, and checks the last poll.
fn check_last_poll<F>(config: toml::Table, startup: StartupExpectations, n_polls: usize, check: F)
where
    F: Fn(&MetricRegistry, &MeasurementBuffer) + Send + Clone + 'static,
{
    let source = SourceName::from_str("nvml", &format!("device_{BUS_ID}"));
    let mut runtime = RuntimeExpectations::new();
    for i in 1..=n_polls {
        let check = check.clone();
        runtime = runtime.test_source(
            source.clone(),
            || (),
            move |ctx| {
                if i == n_polls {
                    check(ctx.metrics(), ctx.measurements());
                }
            },
        );
    }
    let agent = agent::Builder::new(plugin_set(config))
        .with_expectations(startup)
        .with_expectations(runtime)
        .build_and_start()
        .expect("agent should start");
    agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
}

/// Starts the plugin with the default config, and checks the first poll of the GPU source.
fn check_poll<F>(check: F)
where
    F: Fn(&MetricRegistry, &MeasurementBuffer) + Send + Clone + 'static,
{
    check_last_poll(default_config(), StartupExpectations::new(), 1, check);
}

/// Starts the plugin with the default config, and checks the second poll of the GPU source.
/// Some measurements need two polls: (energy, per-process statistics)
fn check_second_poll<F>(check: F)
where
    F: Fn(&MetricRegistry, &MeasurementBuffer) + Send + Clone + 'static,
{
    check_last_poll(default_config(), StartupExpectations::new(), 2, check);
}

/// The points of the metric named `metric`.
fn points<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer, metric: &str) -> Vec<&'a MeasurementPoint> {
    buf.iter()
        .filter(|p| metrics.by_id(&p.metric).unwrap().name == metric)
        .collect()
}

/// The value of the unique point of `metric` that measures the GPU itself (not a process).
fn gpu_value(metrics: &MetricRegistry, buf: &MeasurementBuffer, metric: &str) -> u64 {
    let points: Vec<_> = points(metrics, buf, metric)
        .into_iter()
        .filter(|p| p.consumer == ResourceConsumer::LocalMachine)
        .collect();
    assert_eq!(points.len(), 1, "one point expected for {metric}, got {points:?}");
    points[0].value.as_u64()
}

/// The value of the attribute `key`, as a string.
fn attr(point: &MeasurementPoint, key: &str) -> Option<String> {
    point.attributes().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

/// The value of the unique point of `metric` with the attribute `key = value`.
fn value_with_attr(metrics: &MetricRegistry, buf: &MeasurementBuffer, metric: &str, key: &str, value: &str) -> u64 {
    let points: Vec<_> = points(metrics, buf, metric)
        .into_iter()
        .filter(|p| attr(p, key).as_deref() == Some(value))
        .collect();
    assert_eq!(points.len(), 1, "one point expected for {metric} with {key}={value}");
    points[0].value.as_u64()
}

/// The value of the point of `metric` measured for the process `pid`.
fn process_value(metrics: &MetricRegistry, buf: &MeasurementBuffer, metric: &str, pid: u32) -> u64 {
    let points: Vec<_> = points(metrics, buf, metric)
        .into_iter()
        .filter(|p| p.consumer == ResourceConsumer::Process { pid })
        .collect();
    assert_eq!(points.len(), 1, "one point expected for {metric} and pid {pid}");
    points[0].value.as_u64()
}

mod power {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The instantaneous power is measured in mW.
    #[test]
    fn instant_power() {
        check_poll(|m, buf| {
            assert_eq!(gpu_value(m, buf, "nvml_instant_power"), u64::from(POWER_MW));
        });
    }

    /// The power cap is measured in mW.
    #[test]
    fn power_limit() {
        check_poll(|m, buf| {
            assert_eq!(gpu_value(m, buf, "nvml_power_limit"), u64::from(POWER_LIMIT_MW));
        });
    }
}

mod energy {
    use super::*;
    use pretty_assertions::assert_eq;

    /// energy nothing is measured on the first one.
    #[test]
    fn first_poll() {
        check_poll(|m, buf| {
            assert!(points(m, buf, "nvml_energy_consumption").is_empty());
        });
    }

    /// energy is in mJ.
    #[test]
    fn second_poll() {
        check_second_poll(|m, buf| {
            let points = points(m, buf, "nvml_energy_consumption");
            assert_eq!(points.len(), 1);
            assert_eq!(points[0].value.as_f64(), ENERGY_STEP as f64);
        });
    }
}

mod temperature {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn gpu() {
        check_poll(|m, buf| {
            assert_eq!(gpu_value(m, buf, "nvml_temperature_gpu"), u64::from(TEMPERATURE_C));
        });
    }
}

mod utilization {
    use super::*;
    use pretty_assertions::assert_eq;

    /// utilization of GPU and memory are measured in percent.
    #[test]
    fn major() {
        check_poll(|m, buf| {
            assert_eq!(gpu_value(m, buf, "nvml_gpu_utilization"), u64::from(GPU_UTILIZATION));
            assert_eq!(
                gpu_value(m, buf, "nvml_memory_utilization"),
                u64::from(MEMORY_UTILIZATION)
            );
        });
    }

    /// utilization of  decoder is measured with its sampling period.
    #[test]
    fn decoder() {
        check_poll(|m, buf| {
            assert_eq!(
                gpu_value(m, buf, "nvml_decoder_utilization"),
                u64::from(DECODER_UTILIZATION)
            );
            assert_eq!(
                gpu_value(m, buf, "nvml_decoder_sampling_period"),
                u64::from(DECODER_SAMPLING_US)
            );
        });
    }

    /// utilization of  encoder is measured with its sampling period.
    #[test]
    fn encoder() {
        check_poll(|m, buf| {
            assert_eq!(
                gpu_value(m, buf, "nvml_encoder_utilization"),
                u64::from(ENCODER_UTILIZATION)
            );
            assert_eq!(
                gpu_value(m, buf, "nvml_encoder_sampling_period"),
                u64::from(ENCODER_SAMPLING_US)
            );
        });
    }

    /// The GPU and its processes are measured with the same metric, told apart by the consumer.
    #[test]
    fn gpu_and_processes() {
        check_second_poll(|m, buf| {
            assert_eq!(
                gpu_value(m, buf, "nvml_decoder_utilization"),
                u64::from(DECODER_UTILIZATION)
            );
            assert_eq!(
                process_value(m, buf, "nvml_decoder_utilization", COMPUTE_PID),
                u64::from(PROCESS_DECODER_UTILIZATION)
            );
        });
    }
}

mod memory_info {
    use super::*;
    use pretty_assertions::assert_eq;

    /// free, reserved, total and used memory are measured in bytes.
    #[test]
    fn supported() {
        check_poll(|m, buf| {
            let metric = "nvml_gpu_memory_info";
            assert_eq!(value_with_attr(m, buf, metric, "kind", "free"), MEMORY_FREE_BYTES);
            assert_eq!(
                value_with_attr(m, buf, metric, "kind", "reserved"),
                MEMORY_RESERVED_BYTES
            );
            assert_eq!(value_with_attr(m, buf, metric, "kind", "total"), MEMORY_TOTAL_BYTES);
            assert_eq!(value_with_attr(m, buf, metric, "kind", "used"), MEMORY_USED_BYTES);
        });
    }
}

mod clock_info {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Each clock is measured in Hz (nvml gives MHz).
    #[test]
    fn all_clocks() {
        check_poll(|m, buf| {
            let metric = "nvml_clock_info";
            assert_eq!(
                value_with_attr(m, buf, metric, "clock_type", "SM"),
                u64::from(CLOCK_SM_MHZ) * 1_000_000
            );
            assert_eq!(
                value_with_attr(m, buf, metric, "clock_type", "Video"),
                u64::from(CLOCK_VIDEO_MHZ) * 1_000_000
            );
            assert_eq!(
                value_with_attr(m, buf, metric, "clock_type", "Memory"),
                u64::from(CLOCK_MEMORY_MHZ) * 1_000_000
            );
            assert_eq!(
                value_with_attr(m, buf, metric, "clock_type", "Graphics"),
                u64::from(CLOCK_GRAPHICS_MHZ) * 1_000_000
            );
        });
    }
}

mod processes {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn count() {
        check_poll(|m, buf| {
            assert_eq!(
                gpu_value(m, buf, "nvml_n_compute_processes"),
                u64::from(N_COMPUTE_PROCESSES)
            );
            assert_eq!(
                gpu_value(m, buf, "nvml_n_graphic_processes"),
                u64::from(N_GRAPHICS_PROCESSES)
            );
        });
    }

    #[test]
    fn compute_memory() {
        check_poll(|m, buf| {
            assert_eq!(
                process_value(m, buf, "nvml_used_gpu_memory", COMPUTE_PID),
                COMPUTE_GPU_MEMORY_BYTES
            );
            let point = points(m, buf, "nvml_used_gpu_memory")
                .into_iter()
                .find(|p| p.consumer == ResourceConsumer::Process { pid: COMPUTE_PID })
                .unwrap();
            assert_eq!(attr(point, "context").as_deref(), Some("compute"));
            assert_eq!(point.resource, Resource::Gpu { bus_id: BUS_ID.into() });
        });
    }

    #[test]
    fn graphics_memory_on_partition() {
        check_poll(|m, buf| {
            assert_eq!(
                process_value(m, buf, "nvml_used_gpu_memory", GRAPHICS_PID),
                GRAPHICS_GPU_MEMORY_BYTES
            );
            let point = points(m, buf, "nvml_used_gpu_memory")
                .into_iter()
                .find(|p| p.consumer == ResourceConsumer::Process { pid: GRAPHICS_PID })
                .unwrap();
            assert_eq!(attr(point, "context").as_deref(), Some("graphics"));
            assert_eq!(
                attr(point, "compute_instance_id"),
                Some(GRAPHICS_COMPUTE_INSTANCE_ID.to_string())
            );
            let partition = Resource::GpuPartition {
                parent_id: BUS_ID.into(),
                partition_id: GRAPHICS_PARTITION_ID,
            };
            assert_eq!(point.resource, partition);
        });
    }

    /// The per-process utilization needs the timestamp of a previous poll.
    #[test]
    fn utilization_first_poll() {
        check_poll(|m, buf| {
            assert!(points(m, buf, "nvml_sm_utilization").is_empty());
        });
    }

    /// The utilization of the GPU by each process is measured from the second poll.
    #[test]
    fn utilization_second_poll() {
        check_second_poll(|m, buf| {
            assert_eq!(
                process_value(m, buf, "nvml_sm_utilization", COMPUTE_PID),
                u64::from(PROCESS_SM_UTILIZATION)
            );
            assert_eq!(
                process_value(m, buf, "nvml_memory_utilization", COMPUTE_PID),
                u64::from(PROCESS_MEMORY_UTILIZATION)
            );
            assert_eq!(
                process_value(m, buf, "nvml_encoder_utilization", COMPUTE_PID),
                u64::from(PROCESS_ENCODER_UTILIZATION)
            );
            assert_eq!(
                process_value(m, buf, "nvml_decoder_utilization", COMPUTE_PID),
                u64::from(PROCESS_DECODER_UTILIZATION)
            );
        });
    }
}
