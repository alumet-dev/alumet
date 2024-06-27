use alumet::{
    pipeline::{runtime::ControlHandle, trigger::TriggerSpec},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        util::CounterDiff,
        ConfigTable, Plugin,
    },
};
use anyhow::Context;
use k8s_probe::Metrics;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, time::Duration};

mod cgroup_v2;
mod k8s_probe;
mod parsing_cgroupv2;

use crate::cgroup_v2::CgroupV2MetricFile;
use crate::k8s_probe::K8SProbe;

pub(crate) const CGROUP_MAX_TIME_COUNTER: u64 = u64::MAX;

pub struct K8sPlugin {
    config: Config,
    watcher: Option<RecommendedWatcher>,
    metrics: Option<Metrics>,
}

#[derive(Deserialize, Serialize)]
struct Config {
    path: PathBuf,
    /// Initial interval between two cgroup measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
}

impl AlumetPlugin for K8sPlugin {
    fn name() -> &'static str {
        "k8s"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(K8sPlugin {
            config: config,
            watcher: None,
            metrics: None,
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let v2_used: bool = cgroup_v2::is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"));
        if !v2_used {
            anyhow::bail!("Cgroups v2 are not being used!");
        }
        let metrics_result = Metrics::new(alumet);
        let metrics = metrics_result?;
        self.metrics = Some(metrics.clone());
        let final_li_metric_file: Vec<CgroupV2MetricFile> = cgroup_v2::list_all_k8s_pods_file(&self.config.path)?;

        //Add as a source each pod already present
        for metric_file in final_li_metric_file {
            let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let probe = K8SProbe::new(
                metrics.clone(),
                metric_file,
                counter_tmp_tot,
                counter_tmp_sys,
                counter_tmp_usr,
            )?;
            alumet.add_source(Box::new(probe), TriggerSpec::at_interval(self.config.poll_interval));
        }

        return Ok(());
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn post_pipeline_start(&mut self, pipeline: &mut alumet::pipeline::runtime::RunningPipeline) -> anyhow::Result<()> {
        let control_handle = pipeline.control_handle();
        let plugin_name = self.name().to_owned();

        // let metrics = self.metrics.clone().unwrap();
        let metrics = self.metrics.clone().with_context(|| "Metrics is not available")?;
        let poll_interval = self.config.poll_interval;
        struct PodDetector {
            plugin_name: String,
            metrics: Metrics,
            control_handle: ControlHandle,
            poll_interval: Duration,
        }

        impl EventHandler for PodDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
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
                        match path.extension() {
                            None => {
                                // Case of no extension found --> I will not find cpu.stat file
                                return;
                            }
                            Some(os_str) => match os_str.to_str() {
                                Some("slice") => {
                                    // Case of .slice found --> I will find cpu.stat file
                                    log::debug!(".slice extension found, will continue");
                                }
                                _ => {
                                    // Case of an other extension than .slice is found --> I will not find cpu.stat file
                                    return;
                                }
                            },
                        };
                        if let Some(pod_uid) = path.file_name() {
                            let pod_uid = match pod_uid.to_str() {
                                Some(uid_str) => uid_str,
                                None => {
                                    log::error!("Unable to transform to str");
                                    return;
                                }
                            };
                            // We open a File Descriptor to the newly created file
                            let mut path_cpu = path.clone();
                            let full_name_to_seek = pod_uid.strip_suffix(".slice").unwrap_or(&pod_uid);
                            let parts: Vec<&str> = full_name_to_seek.split("pod").collect();
                            let name_to_seek_raw = *(parts.last().unwrap_or(&full_name_to_seek));
                            let uid_raw = parts.last().unwrap_or(&"No UID found");
                            let uid = format!("pod{}", uid_raw);
                            let name_to_seek = name_to_seek_raw.replace("_", "-");
                            // let (name, ns) = cgroup_v2::get_pod_name(name_to_seek.to_owned());
                            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                                Ok(rt_ok) => rt_ok,
                                Err(err) => {
                                    log::error!("Runtime failed to build and returned an error: {}", err);
                                    return;
                                }
                            };
                            let (name, ns, nd) =
                                match rt.block_on(async { cgroup_v2::get_pod_name(name_to_seek.to_owned()).await }) {
                                    Ok(tuple_found) => tuple_found,
                                    Err(err) => {
                                        log::error!("Block on failed returned an error: {}", err);
                                        return;
                                    }
                                };

                            path_cpu.push("cpu.stat");
                            let file = match File::open(&path_cpu)
                                .with_context(|| format!("failed to open file {}", path_cpu.display()))
                            {
                                Ok(file_opened) => file_opened,
                                Err(err) => {
                                    log::error!("Failed to open file at {:?}, it returned an error: {}", path_cpu, err);
                                    return;
                                }
                            };
                            let metric_file = CgroupV2MetricFile {
                                name: name.to_owned(),
                                path: path_cpu,
                                file: file,
                                uid: uid.to_owned(),
                                namespace: ns.to_owned(),
                                node: nd.to_owned(),
                            };

                            let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                            let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                            let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                            let probe: K8SProbe = match K8SProbe::new(
                                self.metrics.clone(),
                                metric_file,
                                counter_tmp_tot,
                                counter_tmp_sys,
                                counter_tmp_usr,
                            ) {
                                Ok(new_prob) => new_prob,
                                Err(err) => {
                                    log::error!("Failed to create a K8S probe, it returned an error: {}", err);
                                    return;
                                }
                            };

                            // Add the probe to the sources
                            self.control_handle.add_source(
                                self.plugin_name.clone(),
                                pod_uid.to_string(),
                                Box::new(probe),
                                TriggerSpec::at_interval(self.poll_interval),
                            );
                        }
                    }
                }
            }
        }
        let handler = PodDetector {
            plugin_name: plugin_name,
            metrics: metrics,
            control_handle: control_handle,
            poll_interval: poll_interval,
        };

        let mut watcher = notify::recommended_watcher(handler)?;
        watcher.watch(&self.config.path, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let root_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/");
        Self {
            path: root_path,
            poll_interval: Duration::from_secs(1), // 1Hz
        }
    }
}
