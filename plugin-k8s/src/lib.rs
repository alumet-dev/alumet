use alumet::{
    pipeline::trigger::TriggerSpec,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        ConfigTable,
    },
};
use anyhow::Context;
use inotify::{
    Inotify,
    WatchMask,
};
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::{Arc, Mutex}, time::Duration};


mod cgroup_v2;
mod k8s_probe;
mod parsing_cgroupv2;

use crate::cgroup_v2::CgroupV2MetricFile;
use crate::k8s_probe::K8SProbe;

lazy_static! {
    pub static ref INOTIFY_VAR: Arc<Mutex<Option<Inotify>>> = Arc::new(Mutex::new(None));
    pub static ref MAP_FD: Mutex<HashMap<i32, PathBuf>> = Mutex::new(HashMap::new());
}

pub struct K8sPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
struct Config {
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

        if let Ok(mut inotify_guard) = INOTIFY_VAR.lock() {
            *inotify_guard = Some(Inotify::init().expect("Failed to initialize an inotify instance"));
        }    
        Ok(Box::new(K8sPlugin { config: config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let v2_used: bool = cgroup_v2::is_accessible_dir(&PathBuf::from("/sys/fs/cgroup/"));
        if !v2_used {
            anyhow::bail!("Cgroups v2 are not being used!");
        }
        let final_li_metric_file: Vec<CgroupV2MetricFile> = cgroup_v2::list_all_k8s_pods_file("/sys/fs/cgroup/kubepods.slice/")?;
        for directory_to_watch in &final_li_metric_file {
            if let Ok(mut inotify_guard) = INOTIFY_VAR.lock() {
                if let Some(inotify) = inotify_guard.as_mut() {
                    let watch_descriptor = inotify.watches().add(
                        directory_to_watch.path.clone(),
                        WatchMask::CREATE | WatchMask::DELETE,
                    );
                    MAP_FD.lock().unwrap().entry(watch_descriptor.unwrap().get_watch_descriptor_id()).or_insert(directory_to_watch.path.clone());
                }
            }
        }
        let metrics_result = k8s_probe::Metrics::new(alumet);
        let metrics = metrics_result?;
        let probe = K8SProbe::new(metrics.clone(), final_li_metric_file)?;
        alumet.add_source(Box::new(probe), TriggerSpec::at_interval(self.config.poll_interval));
       
        return Ok(());
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if let Ok(mut inotify_guard) = INOTIFY_VAR.lock() {
            if let Some(_inotify) = inotify_guard.as_mut() {
                inotify_guard.take().expect("Trying to close None Inotify").close().expect("Failed to close inotify instance");
            }
            *inotify_guard = None;
        }
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
        }
    }
}
