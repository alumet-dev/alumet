use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    pipeline::elements::source::error::PollError,
    pipeline::Source,
};
use anyhow::Result;

use super::{pod::PodInfos, token::Token};
use crate::cgroupv2::{Cgroupv2Probe, Metrics};
use alumet::measurement::AttributeValue;
use std::{
    path::{Path, PathBuf},
    result::Result::Ok,
    vec,
};
use walkdir::WalkDir;

use super::pod::{get_node_pods_infos, get_uid_from_cgroup_dir, is_cgroup_pod_dir};

pub struct K8SPodProbe {
    cgroupv2: Cgroupv2Probe,
    uid: String,
    name: String,
    namespace: String,
}

impl K8SPodProbe {
    pub fn new(uid: String, name: String, namespace: String, cgroupv2: Cgroupv2Probe) -> Result<Self, anyhow::Error> {
        Ok(Self {
            cgroupv2,
            uid,
            name,
            namespace,
        })
    }
    pub fn source_name(&self) -> String {
        format!("pod:{}_{}_{}", self.namespace, self.name, self.uid)
    }
}

impl Source for K8SPodProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        self.cgroupv2.collect_measurements(timestamp, measurements)?;
        Ok(())
    }
}

pub fn get_all_pod_probes(
    root_directory_path: &Path,
    hostname: String,
    kubernetes_api_url: String,
    token: &Token,
    metrics: Metrics,
) -> Result<Vec<K8SPodProbe>> {
    let mut probes: Vec<K8SPodProbe> = Vec::new();

    if !root_directory_path.exists() {
        return Ok(probes);
    }

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let pods_infos_by_uid = rt.block_on(async { get_node_pods_infos(&hostname, &kubernetes_api_url, token).await })?;

    for entry in WalkDir::new(root_directory_path)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir())
    {
        let path = entry.path();
        if is_cgroup_pod_dir(path) {
            let pod_uid = get_uid_from_cgroup_dir(path)?;

            let empty_pod_infos = PodInfos::default();
            let pod_infos = pods_infos_by_uid.get(&pod_uid).unwrap_or(&empty_pod_infos);

            probes.push(get_pod_probe(
                path.to_path_buf(),
                metrics.clone(),
                pod_uid,
                pod_infos.name.clone(),
                pod_infos.namespace.clone(),
                pod_infos.node.clone(),
            )?);
        }
    }

    Ok(probes)
}

pub fn get_pod_probe(
    path: PathBuf,
    metrics: Metrics,
    uid: String,
    name: String,
    namespace: String,
    node: String,
) -> anyhow::Result<K8SPodProbe> {
    let base_attrs = vec![
        ("uid".to_string(), AttributeValue::String(uid.clone())),
        ("name".to_string(), AttributeValue::String(name.clone())),
        ("namespace".to_string(), AttributeValue::String(namespace.clone())),
        ("node".to_string(), AttributeValue::String(node.clone())),
    ];
    let mut cgroup_probe = Cgroupv2Probe::new_from_cgroup_dir(path, metrics)?;
    cgroup_probe.add_additional_attrs(base_attrs);
    if let Some(cpu_stat) = &mut cgroup_probe.cpu_stat {
        cpu_stat.add_usage_additional_attrs(Vec::new());
    }
    K8SPodProbe::new(uid.to_string(), name, namespace, cgroup_probe)
}
