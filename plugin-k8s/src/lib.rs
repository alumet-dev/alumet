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
    kubernetes_api_url: String,
    hostname: String,
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

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let v2_used: bool = cgroup_v2::is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"));
        if !v2_used {
            anyhow::bail!("Cgroups v2 are not being used!");
        }
        self.metrics = Some(Metrics::new(alumet)?);

        if self.config.hostname == "" {
            let hostname_ostring = gethostname();
            let hostname = hostname_ostring
                .to_str()
                .context("Invalid UTF-8 in Hostname")?
                .to_string();
            self.config.hostname = hostname;
        }

        let final_list_metric_file: Vec<CgroupV2MetricFile> = cgroup_v2::list_all_k8s_pods_file(
            &self.config.path,
            self.config.hostname.clone(),
            self.config.kubernetes_api_url.clone(),
        )?;
        //Add as a source each pod already present
        for metric_file in final_list_metric_file {
            let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
            let probe = K8SProbe::new(
                self.metrics.as_ref().expect("Metrics is not available").clone(),
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

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();

        let metrics: Metrics = self.metrics.clone().expect("Metrics is not available");
        let poll_interval = self.config.poll_interval;
        let kubernetes_api_url = self.config.kubernetes_api_url.clone();
        let hostname = self.config.hostname.to_owned();
        struct PodDetector {
            metrics: Metrics,
            control_handle: ScopedControlHandle,
            poll_interval: Duration,
            kubernetes_api_url: String,
            hostname: String,
        }

        impl EventHandler for PodDetector {
            fn handle_event(&mut self, event: Result<Event, notify::Error>) {
                fn try_handle(
                    pod_detect: &mut PodDetector,
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
                                let full_name_to_seek = pod_uid.strip_suffix(".slice").unwrap_or(&pod_uid);
                                let parts: Vec<&str> = full_name_to_seek.split("pod").collect();
                                let name_to_seek_raw = *(parts.last().unwrap_or(&full_name_to_seek));
                                let uid_raw = parts.last().unwrap_or(&"No UID found");
                                let uid = format!("pod{}", uid_raw);
                                let name_to_seek = name_to_seek_raw.replace("_", "-");
                                // let (name, ns) = cgroup_v2::get_pod_name(name_to_seek.to_owned());
                                let rt = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .context("failed to create local tokio runtime")?;
                                let (name, namespace, node) = rt
                                    .block_on(async {
                                        cgroup_v2::get_pod_name(
                                            name_to_seek.to_owned(),
                                            pod_detect.hostname.clone(),
                                            pod_detect.kubernetes_api_url.clone(),
                                        )
                                        .await
                                    })
                                    .with_context(|| "Block on failed returned an error")?;

                                path_cpu.push("cpu.stat");
                                let file = File::open(&path_cpu)
                                    .with_context(|| format!("failed to open file {}", path_cpu.display()))?;

                                let metric_file = CgroupV2MetricFile {
                                    name: name.to_owned(),
                                    path: path_cpu,
                                    file: file,
                                    uid: uid.to_owned(),
                                    namespace: namespace.to_owned(),
                                    node: node.to_owned(),
                                };

                                let counter_tmp_tot: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                let counter_tmp_usr: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                let counter_tmp_sys: CounterDiff = CounterDiff::with_max_value(CGROUP_MAX_TIME_COUNTER);
                                let probe: K8SProbe = K8SProbe::new(
                                    pod_detect.metrics.clone(),
                                    metric_file,
                                    counter_tmp_tot,
                                    counter_tmp_sys,
                                    counter_tmp_usr,
                                )
                                .with_context(|| format!("Error creating a metric:"))?;

                                // Add the probe to the sources
                                pod_detect
                                    .control_handle
                                    .add_source(
                                        pod_uid,
                                        Box::new(probe),
                                        TriggerSpec::at_interval(pod_detect.poll_interval),
                                    )
                                    .map_err(|e| anyhow::anyhow!("failed to add source for pod {pod_uid}: {e}"))?;
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
            metrics: metrics,
            control_handle: control_handle,
            poll_interval: poll_interval,
            kubernetes_api_url: kubernetes_api_url,
            hostname: hostname,
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
            kubernetes_api_url: String::from("https://127.0.0.1:8080"),
            hostname: String::from(""),
        }
    }
}
