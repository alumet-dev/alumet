use alumet::{
    pipeline::{control::ScopedControlHandle, trigger::TriggerSpec},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        util::CounterDiff,
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
    resources::ResourceConsumer,
};
use anyhow::{anyhow, Context};
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, time::Duration};

use crate::{
    cgroupv2::{Metrics, CGROUP_MAX_TIME_COUNTER},
    is_accessible_dir,
};

use super::{probe::CgroupV2prob, utils::CgroupV2MetricFile};

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
            return Err(anyhow!("Cgroups v2 are not being used!"));
        }
        let metrics_result = Metrics::new(alumet);
        let metrics = metrics_result?;
        self.metrics = Some(metrics.clone());

        let final_list_metric_file = super::utils::list_all_file(&self.config.path)?;

        // Add as a source each pod already present
        for metric_file in final_list_metric_file {
            let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);

            let probe = CgroupV2prob::new(
                metrics.clone(),
                metric_file,
                counter_tmp_tot,
                counter_tmp_sys,
                counter_tmp_usr,
            )?;
            alumet.add_source(Box::new(probe), TriggerSpec::at_interval(self.config.poll_interval));
        }

        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();

        // let metrics = self.metrics.clone().unwrap();
        let metrics = self.metrics.clone().with_context(|| "Metrics is not available")?;
        let poll_interval = self.config.poll_interval;
        struct PodDetector {
            metrics: Metrics,
            control_handle: ScopedControlHandle,
            poll_interval: Duration,
        }

        impl EventHandler for PodDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
                fn try_handle(
                    detector: &mut PodDetector,
                    event: Result<Event, notify::Error>,
                ) -> Result<(), anyhow::Error> {
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
                                if let Some(pod_uid) = path.file_name() {
                                    let mut path_cpu = path.clone();
                                    let mut path_memory = path.clone();

                                    // CPU resource consumer for cpu.stat file in cgroup
                                    let consumer_cpu = ResourceConsumer::ControlGroup {
                                        path: path_cpu
                                            .to_str()
                                            .expect("Path to 'cpu.stat' must be valid UTF8")
                                            .to_string()
                                            .into(),
                                    };
                                    // Memory resource consumer for memory.stat file in cgroup
                                    let consumer_memory = ResourceConsumer::ControlGroup {
                                        path: path_memory
                                            .to_str()
                                            .expect("Path to 'memory.stat' must to be valid UTF8")
                                            .to_string()
                                            .into(),
                                    };

                                    path_cpu.push("cpu.stat");
                                    let file_cpu = File::open(&path_cpu)
                                        .with_context(|| format!("failed to open file {}", path_cpu.display()))?;

                                    path_memory.push("memory.stat");
                                    let file_memory = File::open(&path_memory)
                                        .with_context(|| format!("failed to open file {}", path_memory.display()))?;

                                    let metric_file = CgroupV2MetricFile {
                                        name: pod_uid
                                            .to_str()
                                            .with_context(|| format!("Filename is not valid UTF-8: {:?}", path))
                                            .unwrap_or("ERROR")
                                            .to_string(),
                                        consumer_cpu,
                                        file_cpu,
                                        consumer_memory,
                                        file_memory,
                                    };

                                    let counter_tmp_tot = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                    let counter_tmp_usr = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                    let counter_tmp_sys = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);

                                    let probe: CgroupV2prob = CgroupV2prob::new(
                                        detector.metrics.clone(),
                                        metric_file,
                                        counter_tmp_tot,
                                        counter_tmp_sys,
                                        counter_tmp_usr,
                                    )?;

                                    // Add the probe to the sources
                                    detector
                                        .control_handle
                                        .add_source(
                                            pod_uid.to_str().unwrap(),
                                            Box::new(probe),
                                            TriggerSpec::at_interval(detector.poll_interval),
                                        )
                                        .with_context(|| {
                                            format!("failed to add source for pod {}", pod_uid.to_str().unwrap())
                                        })?;
                                }
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
        let handler = PodDetector {
            control_handle,
            metrics,
            poll_interval,
        };

        let mut watcher = notify::recommended_watcher(handler)?;
        watcher.watch(&self.config.path, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);

        Ok(())
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
