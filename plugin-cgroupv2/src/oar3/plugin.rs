use alumet::{
    pipeline::{
        control::{request, PluginControlHandle},
        elements::source::trigger::TriggerSpec,
    },
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
};
use anyhow::{anyhow, Context};
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

use crate::{cgroupv2::Metrics, is_accessible_dir};

use super::probe::{get_all_job_probes, get_job_probe, OAR3JobProbe};

pub struct OARPlugin {
    config: OAR3Config,
    watcher: Option<RecommendedWatcher>,
    metrics: Option<Metrics>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OAR3Config {
    path: PathBuf,
    /// Initial interval between two cgroup measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl AlumetPlugin for OARPlugin {
    fn name() -> &'static str {
        "OAR3"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(OAR3Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(OARPlugin {
            config,
            watcher: None,
            metrics: None,
        }))
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let v2_used = is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"))?;
        if !v2_used {
            return Err(anyhow!(
                "Cgroups v2 are not being used (/sys/fs/cgroup/ is not accessible)"
            ));
        }
        self.metrics = Some(Metrics::new(alumet)?);

        let job_probes: Vec<OAR3JobProbe> = get_all_job_probes(&self.config.path, self.metrics.clone().unwrap())?;

        for probe in job_probes {
            alumet
                .add_source(
                    &probe.source_name(),
                    Box::new(probe),
                    TriggerSpec::at_interval(self.config.poll_interval),
                )
                .expect("source names should be unique (in the plugin)");
        }

        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();

        // let metrics = self.metrics.clone().unwrap();
        let metrics = self.metrics.clone().with_context(|| "Metrics is not available")?;
        let poll_interval = self.config.poll_interval;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("tokio Runtime should build")?;

        let handler = PodDetector {
            control_handle,
            metrics,
            poll_interval,
            rt,
        };

        let mut watcher = notify::recommended_watcher(handler)?;
        watcher.watch(&self.config.path, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }
}

struct PodDetector {
    metrics: Metrics,
    control_handle: PluginControlHandle,
    poll_interval: Duration,
    rt: tokio::runtime::Runtime,
}

impl EventHandler for PodDetector {
    fn handle_event(&mut self, event: Result<Event, notify::Error>) {
        fn try_handle(detector: &mut PodDetector, event: Result<Event, notify::Error>) -> Result<(), anyhow::Error> {
            // The events look like the following
            // Handle_Event: Ok(Event { kind: Create(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
            // Handle_Event: Ok(Event { kind: Remove(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
            if let Ok(Event {
                kind: EventKind::Create(notify::event::CreateKind::Folder),
                paths,
                ..
            }) = event
            {
                for path in paths {
                    if path.is_dir() {
                        let job_name = path
                            .file_name()
                            .ok_or_else(|| anyhow::anyhow!("No file name found"))?
                            .to_str()
                            .context("Filename is not valid UTF-8")?
                            .to_string();
                        let probe = get_job_probe(path.clone(), detector.metrics.clone(), job_name.clone())?;
                        let source = request::create_one().add_source(
                            &probe.source_name(),
                            Box::new(probe),
                            TriggerSpec::at_interval(detector.poll_interval),
                        );
                        detector
                            .rt
                            .block_on(detector.control_handle.dispatch(source, Duration::from_secs(1)))
                            .with_context(|| format!("failed to add source for pod {job_name}"))?;
                    }
                }
                Ok(())
            } else {
                Ok(())
            }
        }

        if let Err(e) = try_handle(self, event) {
            log::error!("Error try_handle: {}", e);
        }
    }
}

impl Default for OAR3Config {
    fn default() -> Self {
        let root_path = PathBuf::from("/sys/fs/cgroup/");
        if !root_path.exists() {
            log::warn!("Error : Path '{}' not exist.", root_path.display());
        }
        Self {
            path: root_path,
            poll_interval: Duration::from_secs(1), // 1Hz
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::PathBuf;

    // Create a fake plugin structure for oar3 plugin
    fn create_mock_plugin() -> OARPlugin {
        OARPlugin {
            config: OAR3Config {
                path: PathBuf::from("/sys/fs/cgroup/kubepods.slice/"),
                poll_interval: Duration::from_secs(1),
            },
            watcher: None,
            metrics: None,
        }
    }

    // Test `default_config` function of oar3 plugin
    #[test]
    fn test_default_config() {
        let result = OARPlugin::default_config().unwrap();
        assert!(result.is_some(), "result : None");

        let config_table = result.unwrap();
        let config: OAR3Config = deserialize_config(config_table).expect("ERROR : Failed to deserialize config");

        assert_eq!(config.path, PathBuf::from("/sys/fs/cgroup/"));
        assert_eq!(config.poll_interval, Duration::from_secs(1));
    }

    // Test `init` function to initialize oar3 plugin configuration
    #[test]
    fn test_init() -> Result<()> {
        let config_table = serialize_config(OAR3Config::default())?;
        let plugin = OARPlugin::init(config_table)?;
        assert!(plugin.metrics.is_none());
        assert!(plugin.watcher.is_none());
        Ok(())
    }

    // Test `stop` function to stop oar3 plugin
    #[test]
    fn test_stop() {
        let mut plugin = create_mock_plugin();
        let result = plugin.stop();
        assert!(result.is_ok(), "Stop should complete without errors.");
    }
}
