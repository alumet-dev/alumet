//! Integration tests of the "minimal" mode, which only measures the power and estimates the energy from it.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp},
    metrics::registry::MetricRegistry,
    pipeline::naming::SourceName,
    plugin::{PluginMetadata, rust::AlumetPlugin},
    resources::ResourceConsumer,
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};
use nvml_wrapper::error::NvmlError;
use plugin_nvidia_nvml::{
    NvmlPlugin,
    mock::{MockNvmlDevice, MockNvmlLib, NvmlProvider},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const BUS_ID: &str = "00000000:02:00.0"; // PCI bus id of the mock GPU
const POWER_MW: u32 = 190_000; // power of the GPU with a constant power
const POWER_STEP_MW: u32 = 10_000; // increase of the power at each call, for the GPU with a rising power

/// A GPU whose `power_usage` returns the values given by `power`, and which supports
/// the temperature if `temperature` is not `None`.
///
/// The other features are not supported. They are only called once, by the features detection.
fn device(power: impl FnMut() -> Result<u32, NvmlError> + Send + 'static, temperature: Option<u32>) -> MockNvmlDevice {
    let mut device = MockNvmlDevice::new();
    device.expect_name().return_const(String::from("Mock GPU"));
    device.expect_bus_id().return_const(String::from(BUS_ID));
    device.expect_power_usage().returning(power);
    device
        .expect_total_energy_consumption()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_temperature()
        .returning(move |_| temperature.ok_or(NvmlError::NotSupported));
    device
        .expect_utilization_rates()
        .returning(|| Err(NvmlError::NotSupported));
    device.expect_memory_info().returning(|| Err(NvmlError::NotSupported));
    device
        .expect_decoder_utilization()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_encoder_utilization()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_process_utilization_stats()
        .returning(|_| Err(NvmlError::NotSupported));
    device
        .expect_running_compute_processes()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_running_compute_processes_v2()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_running_graphics_processes()
        .returning(|| Err(NvmlError::NotSupported));
    device
        .expect_running_graphics_processes_v2()
        .returning(|| Err(NvmlError::NotSupported));
    device.expect_clock_info().returning(|_| Err(NvmlError::NotSupported));
    device
        .expect_power_management_limit()
        .returning(|| Err(NvmlError::NotSupported));
    device.expect_gpm_support().returning(|| Ok(false));
    device
}

/// A NVML library with one GPU.
fn lib(device: impl Fn() -> MockNvmlDevice + Send + 'static) -> MockNvmlLib {
    let mut lib = MockNvmlLib::new();
    lib.expect_device_count().returning(|| Ok(1));
    lib.expect_device_by_index().returning(move |_| Ok(device()));
    lib
}

/// A GPU whose power is always `POWER_MW`.
struct ConstantPower;

impl NvmlProvider for ConstantPower {
    type Lib = MockNvmlLib;

    fn get() -> anyhow::Result<MockNvmlLib> {
        Ok(lib(|| device(|| Ok(POWER_MW), None)))
    }
}

/// A GPU whose power increases by `POWER_STEP_MW` at each call.
struct RisingPower;

impl NvmlProvider for RisingPower {
    type Lib = MockNvmlLib;

    fn get() -> anyhow::Result<MockNvmlLib> {
        Ok(lib(|| {
            let mut power = POWER_MW;
            device(
                move || {
                    power += POWER_STEP_MW;
                    Ok(power)
                },
                None,
            )
        }))
    }
}

/// A GPU that does not support `power_usage`, but supports the temperature
/// (without any supported feature, the GPU would be ignored by the detection).
struct NoPower;

impl NvmlProvider for NoPower {
    type Lib = MockNvmlLib;

    fn get() -> anyhow::Result<MockNvmlLib> {
        Ok(lib(|| device(|| Err(NvmlError::NotSupported), Some(57))))
    }
}

/// The plugin `NvmlPlugin<P>`, configured in minimal mode.
fn plugin_set<P: NvmlProvider + 'static>() -> PluginSet {
    let mut config = NvmlPlugin::<P>::default_config().unwrap().unwrap().0;
    config["mode"] = toml::Value::String(String::from("minimal"));
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<NvmlPlugin<P>>(),
        enabled: true,
        config: Some(config),
    });
    plugins
}

fn source() -> SourceName {
    SourceName::from_str("nvml", &format!("device_{BUS_ID}"))
}

/// Runs the agent with the given expectations, until it stops.
fn run<P: NvmlProvider + 'static>(startup: StartupExpectations, runtime: RuntimeExpectations) {
    let agent = agent::Builder::new(plugin_set::<P>())
        .with_expectations(startup)
        .with_expectations(runtime)
        .build_and_start()
        .expect("agent should start");
    agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
}

/// Starts the plugin with the GPU of `P`, polls the source `n_polls` times, and checks the last poll.
fn check_last_poll<P, F>(n_polls: usize, check: F)
where
    P: NvmlProvider + 'static,
    F: Fn(&MetricRegistry, &MeasurementBuffer) + Send + Clone + 'static,
{
    let mut runtime = RuntimeExpectations::new();
    for i in 1..=n_polls {
        let check = check.clone();
        runtime = runtime.test_source(
            source(),
            || (),
            move |ctx| {
                if i == n_polls {
                    check(ctx.metrics(), ctx.measurements());
                }
            },
        );
    }
    run::<P>(StartupExpectations::new(), runtime);
}

/// The points of the metric named `metric`.
fn points<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer, metric: &str) -> Vec<&'a MeasurementPoint> {
    buf.iter()
        .filter(|p| metrics.by_id(&p.metric).unwrap().name == metric)
        .collect()
}

/// The unique point of `metric`.
fn point<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer, metric: &str) -> &'a MeasurementPoint {
    let points = points(metrics, buf, metric);
    assert_eq!(points.len(), 1, "one point expected for {metric}, got {points:?}");
    points[0]
}

mod startup {
    use super::*;

    /// In minimal mode, only the power and energy metrics are registered.
    #[test]
    fn metrics() {
        let startup = StartupExpectations::new()
            .expect_metric::<f64>("nvml_energy_consumption", PrefixedUnit::milli(Unit::Joule))
            .expect_metric::<u64>("nvml_instant_power", PrefixedUnit::milli(Unit::Watt))
            .expect_source("nvml", &format!("device_{BUS_ID}"));
        let runtime = RuntimeExpectations::new().test_source(source(), || (), |_| ());
        run::<ConstantPower>(startup, runtime);
    }

    /// The minimal mode cannot be used on a GPU that does not support `power_usage`.
    #[test]
    fn without_power_usage() {
        let agent = agent::Builder::new(plugin_set::<NoPower>()).build_and_start();
        assert!(agent.is_err(), "the plugin should fail to start");
    }
}

mod instant_power {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The power of the GPU is measured in mW, and it is the only measurement on the first poll.
    #[test]
    fn first_poll() {
        check_last_poll::<ConstantPower, _>(1, |m, buf| {
            assert_eq!(buf.len(), 1);
            let point = point(m, buf, "nvml_instant_power");
            assert_eq!(point.value.as_u64(), u64::from(POWER_MW));
            assert_eq!(point.consumer, ResourceConsumer::LocalMachine);
        });
    }
}

mod energy {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The energy needs two power measurements: nothing is estimated on the first poll.
    #[test]
    fn first_poll() {
        check_last_poll::<RisingPower, _>(1, |m, buf| {
            assert!(points(m, buf, "nvml_energy_consumption").is_empty());
        });
    }

    /// When the power has not changed (the NVML registry is not updated yet), no energy is estimated.
    #[test]
    fn constant_power() {
        check_last_poll::<ConstantPower, _>(2, |m, buf| {
            assert!(points(m, buf, "nvml_energy_consumption").is_empty());
            assert_eq!(point(m, buf, "nvml_instant_power").value.as_u64(), u64::from(POWER_MW));
        });
    }

    /// The energy is estimated in mJ from the two last powers: (P1 + P2) / 2 × Δt.
    #[test]
    fn rising_power() {
        // the power and timestamp measured on the first poll
        let first: Arc<Mutex<Option<(u64, Timestamp)>>> = Arc::default();
        let first_in = Arc::clone(&first);
        let runtime = RuntimeExpectations::new()
            .test_source(
                source(),
                || (),
                move |ctx| {
                    let power = point(ctx.metrics(), ctx.measurements(), "nvml_instant_power");
                    *first_in.lock().unwrap() = Some((power.value.as_u64(), power.timestamp));
                },
            )
            .test_source(
                source(),
                || (),
                move |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    let (p1, t1) = first.lock().unwrap().expect("first poll should have been checked");
                    let power = point(m, buf, "nvml_instant_power");
                    let (p2, t2) = (power.value.as_u64(), power.timestamp);
                    assert_eq!(p2, p1 + u64::from(POWER_STEP_MW));

                    let energy = point(m, buf, "nvml_energy_consumption");
                    let dt = t2.duration_since(t1).unwrap().as_secs_f64();
                    let expected = (p1 + p2) as f64 / 2.0 * dt;
                    let actual = energy.value.as_f64();
                    assert!((actual - expected).abs() < 1e-6, "energy {actual} != {expected} mJ");
                    assert_eq!(energy.timestamp, t2);
                },
            );
        run::<RisingPower>(StartupExpectations::new(), runtime);
    }
}
