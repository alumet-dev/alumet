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
use gethostname::gethostname;
use notify::{Event, EventHandler, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

use crate::{cgroupv2::Metrics, is_accessible_dir};

use super::{
    pod::{get_pod_infos, get_uid_from_cgroup_dir, is_cgroup_pod_dir},
    probe::{get_all_pod_probes, get_pod_probe, K8SPodProbe},
    token::{Token, TokenRetrieval},
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

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let v2_used = is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"))?;
        if !v2_used {
            return Err(anyhow!(
                "Cgroups v2 are not being used (/sys/fs/cgroup/ is not accessible)"
            ));
        }
        self.metrics = Some(Metrics::new(alumet)?);

        if self.config.hostname.is_empty() {
            let hostname_ostring = gethostname();
            self.config.hostname = hostname_ostring
                .to_str()
                .with_context(|| format!("Invalid UTF-8 in hostname: {hostname_ostring:?}"))?
                .to_string();
        }

        let pod_probes: Vec<K8SPodProbe> = get_all_pod_probes(
            &self.config.path,
            self.config.hostname.clone(),
            self.config.kubernetes_api_url.clone(),
            &Token::new(self.config.token_retrieval.clone()),
            self.metrics.clone().unwrap(),
        )?;

        // Add as a source each pod already present
        for probe in pod_probes {
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

        let metrics = self.metrics.clone().expect("Metrics is not available");
        let poll_interval = self.config.poll_interval;
        let kubernetes_api_url = self.config.kubernetes_api_url.clone();
        let hostname = self.config.hostname.to_owned();
        let token_retrieval = self.config.token_retrieval.clone();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .context("tokio Runtime should build")?;

        let handler = PodDetector {
            metrics,
            control_handle,
            poll_interval,
            kubernetes_api_url,
            hostname,
            token: Token::new(token_retrieval),
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
    kubernetes_api_url: String,
    hostname: String,
    token: Token,
    rt: tokio::runtime::Runtime,
}

impl EventHandler for PodDetector {
    fn handle_event(&mut self, event: Result<Event, notify::Error>) {
        if let Ok(Event {
            kind: EventKind::Create(notify::event::CreateKind::Folder),
            paths,
            ..
        }) = event
        {
            for path in paths {
                if let Err(e) = self.handle_event_on_path(path.clone()) {
                    log::error!("Unable to handle event on {}: {}", path.display(), e);
                }
            }
        } else if let Err(e) = event {
            log::error!("watch error: {:?}", e);
        }
    }
}

impl PodDetector {
    fn handle_event_on_path(&self, path: PathBuf) -> anyhow::Result<()> {
        // The events look like the following
        // Handle_Event: Ok(Event { kind: Create(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
        // Handle_Event: Ok(Event { kind: Remove(Folder), paths: ["/sys/fs/cgroup/kubepods.slice/kubepods-besteffort.slice/TESTTTTT"], attr:tracker: None, attr:flag: None, attr:info: None, attr:source: None })
        if is_cgroup_pod_dir(&path) {
            let pod_uid = get_uid_from_cgroup_dir(&path)?;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("failed to create local tokio runtime")?;
            let pod_infos = rt
                .block_on(async {
                    get_pod_infos(&pod_uid, &self.hostname, &self.kubernetes_api_url, &self.token).await
                })
                .with_context(|| "Block on failed returned an error")?;

            let probe = get_pod_probe(
                path.clone(),
                self.metrics.clone(),
                pod_uid.clone(),
                pod_infos.name.clone(),
                pod_infos.namespace.clone(),
                pod_infos.node.clone(),
            )?;

            // Add the probe to the sources
            let source = request::create_one().add_source(
                &probe.source_name(),
                Box::new(probe),
                TriggerSpec::at_interval(self.poll_interval),
            );
            self.rt
                .block_on(self.control_handle.dispatch(source, Duration::from_secs(1)))
                .with_context(|| format!("failed to add source for pod {pod_uid}"))?;
        }
        Ok(())
    }
}

impl Default for K8sConfig {
    fn default() -> Self {
        let root_path = PathBuf::from("/sys/fs/cgroup/kubepods.slice/");
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::path::PathBuf;

    // Create a fake plugin structure for k8s plugin
    fn fake_k8s() -> K8sPlugin {
        K8sPlugin {
            config: K8sConfig {
                path: PathBuf::from("/sys/fs/cgroup/kubepods.slice/"),
                poll_interval: Duration::from_secs(1),
                kubernetes_api_url: String::from("https://127.0.0.1:8080"),
                hostname: String::from("test-hostname"),
                token_retrieval: TokenRetrieval::Kubectl,
            },
            watcher: None,
            metrics: None,
        }
    }

    // Test `default_config` function of k8s plugin
    #[test]
    fn test_default_config() {
        let result = K8sPlugin::default_config().unwrap();
        assert!(result.is_some(), "result = None");

        let config_table = result.unwrap();
        let config: K8sConfig = deserialize_config(config_table).expect("Failed to deserialize config");

        assert_eq!(config.path, PathBuf::from("/sys/fs/cgroup/kubepods.slice/"));
        assert_eq!(config.poll_interval, Duration::from_secs(1));
        assert_eq!(config.kubernetes_api_url, "https://127.0.0.1:8080");
        assert_eq!(config.hostname, "");
        assert_eq!(config.token_retrieval, TokenRetrieval::Kubectl);
    }

    // Test `init` function to initialize k8s plugin configuration
    #[test]
    fn test_init() -> Result<()> {
        let config_table = serialize_config(K8sConfig::default())?;
        let plugin = K8sPlugin::init(config_table)?;
        assert_eq!(plugin.config.kubernetes_api_url, "https://127.0.0.1:8080");
        assert!(plugin.metrics.is_none());
        assert!(plugin.watcher.is_none());
        Ok(())
    }

    // Test `stop` function to stop k8s plugin
    #[test]
    fn test_stop() {
        let mut plugin = fake_k8s();
        let result = plugin.stop();
        assert!(result.is_ok(), "Stop should complete without errors.");
    }
}
