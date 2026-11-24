use anyhow::{Context, anyhow};
use nvml_wrapper::enums::gpm::GpmMetricId;
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

/// Mocks of the NVML library, to test the plugin without a GPU.
#[cfg(feature = "mock")]
pub mod mock {
    pub use crate::nvml::{MockNvmlDevice, MockNvmlLib, NvmlProvider};
}

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
            Mode::Full => SourceProvider::Full(FullMetrics::new(alumet, &self.config.gpm_metrics)?),
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

    gpm_metrics: Vec<GpmMetricId>,
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
            gpm_metrics: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use alumet::{
        agent::plugin::{PluginInfo, PluginSet},
        plugin::PluginMetadata,
    };

    use crate::nvml::MockNvmlLib;

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

    fn mock_no_gpu(mock: &mut MockNvmlLib) {
        mock.expect_device_count().returning(|| Ok(0)).times(1);
        mock.expect_device_by_index().never();
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
    fn check_default_config() {
        let config = serialize_config(super::Config::default()).unwrap();
        let rendered: String = config
            .0
            .iter()
            .map(|tuple| format!("{}: {}\n", tuple.0, tuple.1))
            .collect();
        insta::assert_snapshot!(rendered, @r#"
        poll_interval: "1s"
        flush_interval: "5s"
        skip_failed_devices: true
        mode: "full"
        gpm_metrics: []
        "#);
    }
}
