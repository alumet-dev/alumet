//! Integration tests of the GPM metrics, with a mock GPU that only supports GPM.

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
    test::{RuntimeExpectations, StartupExpectations},
    units::Unit,
};
use nvml_wrapper::{
    enums::gpm::GpmMetricId::{self, *},
    error::NvmlError,
    struct_wrappers::gpm::{GpmMetricInfo, GpmMetricResult},
};
use nvml_wrapper_sys::bindings::nvmlGpmSample_t;
use plugin_nvidia_nvml::{
    NvmlPlugin,
    mock::{MockNvmlDevice, MockNvmlLib, NvmlProvider},
};

const TIMEOUT: Duration = Duration::from_secs(5);
const BUS_ID: &str = "00000000:01:00.0";

/// A GPU that only supports GPM (and memory_info), and returns a value for each requested GPM metric.
fn gpm_only_device() -> MockNvmlDevice {
    let mut device = MockNvmlDevice::new();
    device.expect_name().return_const(String::from("Mock GPU"));
    device.expect_bus_id().return_const(String::from(BUS_ID));
    device
        .expect_total_energy_consumption()
        .returning(|| Err(NvmlError::NotSupported));
    device.expect_power_usage().returning(|| Err(NvmlError::NotSupported));
    device.expect_temperature().returning(|_| Err(NvmlError::NotSupported));
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
    device.expect_gpm_support().returning(|| Ok(true));
    device
        .expect_create_gpm_sample()
        .returning(|| std::ptr::null_mut::<()>() as nvmlGpmSample_t);
    device.expect_drop_gpm_sample().returning(|_| ());
    device.expect_gpm_metrics_get().returning(|_, _, ids| {
        let result = |metric_id| {
            Ok(GpmMetricResult {
                metric_id,
                value: 42.0,
                metric_info: GpmMetricInfo {
                    short_name: String::new(),
                    long_name: String::new(),
                    unit: String::new(),
                },
            })
        };
        Ok(ids.iter().copied().map(result).collect())
    });
    device
}

/// Provides a NVML library with one GPM-only GPU.
struct GpmProvider;

impl NvmlProvider for GpmProvider {
    type Lib = MockNvmlLib;

    fn get() -> anyhow::Result<MockNvmlLib> {
        let mut nvml = MockNvmlLib::new();
        nvml.expect_device_count().returning(|| Ok(1));
        nvml.expect_device_by_index().returning(|_| Ok(gpm_only_device()));
        Ok(nvml)
    }
}

type Plugin = NvmlPlugin<GpmProvider>;

/// Every GPM metric id known by nvml-wrapper.
fn all_gpm_ids() -> Vec<GpmMetricId> {
    (0..=u8::MAX as u32)
        .filter_map(|i| GpmMetricId::try_from(i).ok())
        .collect()
}

/// The plugin, configured to measure the given GPM metrics.
fn plugin_set(gpm_metrics: &[GpmMetricId]) -> PluginSet {
    let mut config = Plugin::default_config().unwrap().unwrap().0;
    let ids = gpm_metrics.iter().map(|id| toml::Value::String(format!("{id:?}")));
    config["gpm_metrics"] = toml::Value::Array(ids.collect());
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<Plugin>(),
        enabled: true,
        config: Some(config),
    });
    plugins
}

/// Starts the plugin with the given GPM metrics, and checks the first poll of the GPU source.
fn check_poll<F>(gpm_metrics: &[GpmMetricId], startup: StartupExpectations, check: F)
where
    F: Fn(&MetricRegistry, &MeasurementBuffer) + Send + 'static,
{
    let source = SourceName::from_str("nvml", &format!("device_{BUS_ID}"));
    let runtime =
        RuntimeExpectations::new().test_source(source, || (), move |ctx| check(ctx.metrics(), ctx.measurements()));
    let agent = agent::Builder::new(plugin_set(gpm_metrics))
        .with_expectations(startup)
        .with_expectations(runtime)
        .build_and_start()
        .expect("agent should start");
    agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
}

/// The GPM points of a measurement buffer.
fn gpm_points<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer) -> Vec<&'a MeasurementPoint> {
    buf.iter()
        .filter(|p| metrics.by_id(&p.metric).unwrap().name.starts_with("nvml_gpm_"))
        .collect()
}

fn attr(point: &MeasurementPoint, key: &str) -> Option<String> {
    point.attributes().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

mod create_gpm_metrics {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Each family of GPM ids is registered with the expected name and unit.
    #[test]
    fn names_and_units() {
        let ids = [
            GraphicsUtil,
            SmUtil,
            SmOccupancy,
            AnyTensorUtil,
            IntegerUtil,
            DramBwUtil,
            PcieTxPerSec,
            Nvdec0Util,
            Nvjpg0Util,
            Nvofa0Util,
            NvlinkTotalRxPerSec,
        ];
        let startup = StartupExpectations::new()
            .expect_metric::<u64>("nvml_gpm_graphics_util", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_sm_util", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_sm_occupancy", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_tensor_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_precision_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_dram_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_pcie_throughput", Unit::Byte)
            .expect_metric::<u64>("nvml_gpm_nvdec_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_nvjpg_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_nvofa_utilization", Unit::Percent)
            .expect_metric::<u64>("nvml_gpm_nvlink_throughput", Unit::Byte);
        check_poll(&ids, startup, |_, _| ());
    }

    /// The ids of a same family are measured with the same metric, and told apart by their attributes.
    #[test]
    fn same_family() {
        check_poll(
            &[AnyTensorUtil, ImmaTensorUtil],
            StartupExpectations::new(),
            |m, buf| {
                let points = gpm_points(m, buf);
                assert_eq!(points.len(), 2);
                assert_eq!(points[0].metric, points[1].metric);
                let mut ops: Vec<_> = points.iter().map(|p| attr(p, "operation_type").unwrap()).collect();
                ops.sort();
                assert_eq!(ops, ["any", "imma"]);
            },
        );
    }

    /// Every GPM id known by nvml-wrapper is registered and measured.
    #[test]
    fn all_ids() {
        let ids = all_gpm_ids();
        let n_ids = ids.len();
        check_poll(&ids, StartupExpectations::new(), move |m, buf| {
            assert_eq!(gpm_points(m, buf).len(), n_ids);
        });
    }
}

mod add_gpm_metrics {
    use super::*;

    /// No GPM metric is registered nor measured when none is requested.
    #[test]
    fn none_requested() {
        check_poll(&[], StartupExpectations::new(), |m, buf| {
            assert!(m.iter().all(|(_, metric)| !metric.name.starts_with("nvml_gpm_")));
            assert!(gpm_points(m, buf).is_empty());
        });
    }
}

mod full_metrics {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Only the requested GPM metrics are measured, with the value returned by NVML.
    #[test]
    fn gpm_metrics_ids() {
        check_poll(&[SmOccupancy, Fp16Util], StartupExpectations::new(), |m, buf| {
            let points = gpm_points(m, buf);
            let mut names: Vec<_> = points
                .iter()
                .map(|p| m.by_id(&p.metric).unwrap().name.as_str())
                .collect();
            names.sort();
            assert_eq!(names, ["nvml_gpm_precision_utilization", "nvml_gpm_sm_occupancy"]);
            assert!(points.iter().all(|p| p.value.as_u64() == 42));
        });
    }
}
