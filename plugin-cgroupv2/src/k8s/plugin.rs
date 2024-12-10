//! # plugin file for k8s module of cgroupv2 plugin
//!
//! This module provides functionality for interacting with Kubernetes api.
use alumet::{
    pipeline::{control::ScopedControlHandle, trigger::TriggerSpec},
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        util::CounterDiff,
        AlumetPluginStart, AlumetPostStart, ConfigTable,
    },
};
use anyhow::Context;
use gethostname::gethostname;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::PathBuf, time::Duration};

use crate::{
    cgroupv2::{Metrics, CGROUP_MAX_TIME_COUNTER},
    k8s::utils::get_pod_name,
};

use super::{
    probe::K8SProbe,
    token::Token,
    utils::{self, CgroupV2MetricFile},
};

pub struct K8sPlugin {
    config: K8sConfig,
    watcher: Option<RecommendedWatcher>,
    metrics: Option<Metrics>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct K8sConfig {
    path: PathBuf,
    /// Initial interval between two cgroup measurements.
    #[serde(with = "humantime_serde")]
    poll_interval: Duration,
    kubernetes_api_url: String,
    hostname: String,
    /// Way to retrieve the k8s API token.
    token_retrieval: TokenRetrieval,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum TokenRetrieval {
    Kubectl,
    File,
}

impl AlumetPlugin for K8sPlugin {
    fn name() -> &'static str {
        "k8s"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(K8sConfig::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config).context("invalid config")?;
        Ok(Box::new(K8sPlugin {
            config,
            watcher: None,
            metrics: None,
        }))
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let v2_used: bool = super::utils::is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/system.slice/docker-3525f4fbfe549c7eb566acec93e89a57124659a55e0a16531e47ff899cf6bc49.scope/"))?;
        //let v2_used: bool = super::utils::is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"))?;
        if !v2_used {
            anyhow::bail!("Cgroups v2 are not being used!");
        }
        self.metrics = Some(Metrics::new(alumet)?);

        if self.config.hostname.is_empty() {
            let hostname_ostring = gethostname();
            let hostname = hostname_ostring
                .to_str()
                .with_context(|| format!("Invalid UTF-8 in hostname: {hostname_ostring:?}"))?
                .to_string();
            self.config.hostname = hostname;
        }

        let final_list_metric_file: Vec<CgroupV2MetricFile> = utils::list_all_k8s_pods_file(
            &self.config.path,
            self.config.hostname.clone(),
            self.config.kubernetes_api_url.clone(),
            &Token::new(self.config.token_retrieval.clone()),
        )?;

        // Add as a source each pod already present
        for metric_file in final_list_metric_file {
            let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(crate::cgroupv2::CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(crate::cgroupv2::CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(crate::cgroupv2::CGROUP_MAX_TIME_COUNTER);

            let probe = K8SProbe::new(
                self.metrics.as_ref().expect("Metrics is not available").clone(),
                metric_file,
                counter_tmp_tot,
                counter_tmp_sys,
                counter_tmp_usr,
            )?;
            alumet.add_source(Box::new(probe), TriggerSpec::at_interval(self.config.poll_interval));
        }

        Ok(())
    }

    #[cfg(not(tarpaulin_include))]
    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();

        let metrics: Metrics = self.metrics.clone().expect("Metrics is not available");
        let poll_interval = self.config.poll_interval;
        let kubernetes_api_url = self.config.kubernetes_api_url.clone();
        let hostname = self.config.hostname.to_owned();
        let token_retrieval = self.config.token_retrieval.clone();

        struct PodDetector {
            metrics: Metrics,
            control_handle: ScopedControlHandle,
            poll_interval: Duration,
            kubernetes_api_url: String,
            hostname: String,
            token: Token,
        }

        impl EventHandler for PodDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
                fn try_handle(
                    detector: &mut PodDetector,
                    event: Result<Event, notify::Error>,
                ) -> Result<(), anyhow::Error> {
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
                                    return Ok(());
                                }
                                Some(os_str) => match os_str.to_str() {
                                    Some("slice") => {
                                        // Case of .slice found --> I will find cpu.stat file
                                        log::debug!(".slice extension found, will continue");
                                    }
                                    _ => {
                                        // Case of an other extension than .slice is found --> I will not find cpu.stat file
                                        return Ok(());
                                    }
                                },
                            };
                            if let Some(pod_uid) = path.file_name() {
                                let pod_uid = pod_uid.to_str().expect("Can't retrieve the pod uid value");

                                // We open a File Descriptor to the newly created file
                                let mut path_cpu = path.clone();
                                let mut path_memory = path.clone();
                                let full_name_to_seek = pod_uid.strip_suffix(".slice").unwrap_or(pod_uid);
                                let parts: Vec<&str> = full_name_to_seek.split("pod").collect();
                                let name_to_seek_raw = *(parts.last().unwrap_or(&full_name_to_seek));
                                let uid_raw = parts.last().unwrap_or(&"No UID found");
                                let uid = format!("pod{}", uid_raw);
                                let name_to_seek = name_to_seek_raw.replace('_', "-");

                                let rt = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .context("failed to create local tokio runtime")?;
                                let (name, namespace, node) = rt
                                    .block_on(async {
                                        get_pod_name(
                                            &name_to_seek,
                                            &detector.hostname,
                                            &detector.kubernetes_api_url,
                                            &detector.token,
                                        )
                                        .await
                                    })
                                    .with_context(|| "Block on failed returned an error")?;

                                path_cpu.push("cpu.stat");
                                let file_cpu = File::open(&path_cpu)
                                    .with_context(|| format!("failed to open file {}", path_cpu.display()))?;

                                path_memory.push("memory.stat");
                                let file_memory = File::open(&path_memory)
                                    .with_context(|| format!("failed to open file {}", path_memory.display()))?;

                                let metric_file = CgroupV2MetricFile {
                                    name: name.to_owned(),
                                    path_cpu,
                                    file_cpu,
                                    path_memory,
                                    file_memory,
                                    uid: uid.to_owned(),
                                    namespace: namespace.to_owned(),
                                    node: node.to_owned(),
                                };

                                let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);

                                let probe: K8SProbe = K8SProbe::new(
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
                                        pod_uid,
                                        Box::new(probe),
                                        TriggerSpec::at_interval(detector.poll_interval),
                                    )
                                    .with_context(|| format!("failed to add source for pod {pod_uid}"))?;
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
            metrics,
            control_handle,
            poll_interval,
            kubernetes_api_url,
            hostname,
            token: Token::new(token_retrieval),
        };

        let mut watcher = notify::recommended_watcher(handler)?;
        watcher.watch(&self.config.path, RecursiveMode::Recursive)?;

        self.watcher = Some(watcher);

        Ok(())
    }
}

impl Default for K8sConfig {
    fn default() -> Self {
        let root_path = PathBuf::from("/sys/fs/cgroup/system.slice/docker-3525f4fbfe549c7eb566acec93e89a57124659a55e0a16531e47ff899cf6bc49.scope/kubepods.slice/");
        //let root_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/");
        if !root_path.exists() {
            log::warn!("Error : Path '{}' not exist.", root_path.display());
        }
        Self {
            path: root_path,
            poll_interval: Duration::from_secs(1), // 1Hz
            kubernetes_api_url: String::from("https://127.0.0.1:8080"),
            hostname: String::from(""),
            token_retrieval: TokenRetrieval::Kubectl,
        }
    }
}

// ------------------ //
// --- UNIT TESTS --- //
// ------------------ //
#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::PathBuf;

    // Create a fake plugin structure for k8s plugin
    fn fake_k8s() -> K8sPlugin {
        K8sPlugin {
            config: K8sConfig {
                path: PathBuf::from("/sys/fs/cgroup/system.slice/docker-3525f4fbfe549c7eb566acec93e89a57124659a55e0a16531e47ff899cf6bc49.scope/kubepods.slice/"),
                poll_interval: Duration::from_secs(1),
                kubernetes_api_url: String::from("https://127.0.0.1:8080"),
                hostname: String::from("test-hostname"),
                token_retrieval: TokenRetrieval::Kubectl,
            },
            watcher: None,
            metrics: None,
        }
    }

    // Test default configuration of k8s plugin
    #[test]
    fn test_default_config() {
        let result: Option<ConfigTable> = K8sPlugin::default_config().unwrap();
        assert!(result.is_some(), "Expected Some(ConfigTable): result = None");

        let config_table: ConfigTable = result.unwrap();
        let config: K8sConfig = deserialize_config(config_table).expect("Failed to deserialize config");

        assert_eq!(config.path, PathBuf::from("/sys/fs/cgroup/system.slice/docker-3525f4fbfe549c7eb566acec93e89a57124659a55e0a16531e47ff899cf6bc49.scope/kubepods.slice/"));
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.kubernetes_api_url, "https://127.0.0.1:8080");
        assert_eq!(config.hostname, "");
        assert_eq!(config.token_retrieval, TokenRetrieval::Kubectl);
    }

    // Test `init` function to initialize k8s plugin configuration
    #[test]
    fn test_init() -> Result<()> {
        let config_table: ConfigTable = serialize_config(K8sConfig::default())?;
        let plugin: Box<K8sPlugin> = K8sPlugin::init(config_table)?;
        assert_eq!(plugin.config.kubernetes_api_url, "https://127.0.0.1:8080");
        assert!(plugin.metrics.is_none());
        assert!(plugin.watcher.is_none());
        Ok(())
    }

    // Test `stop` function to stop k8s plugin
    #[test]
    fn test_stop() {
        let mut plugin: K8sPlugin = fake_k8s();
        let result: std::result::Result<(), anyhow::Error> = plugin.stop();
        assert!(result.is_ok(), "Stop should complete without errors.");
    }
}
