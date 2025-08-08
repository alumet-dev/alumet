use alumet::{measurement::AttributeValue, pipeline::elements::source::trigger::TriggerSpec};

use crate::common::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    metrics::{AugmentedMetrics, Metrics},
};

#[derive(Clone)]
pub struct SourceSetup {
    pub trigger: TriggerSpec,
    pub k8s_pods: super::pods::AutoNodePodRegistry,
}

impl CgroupSetupCallback for SourceSetup {
    fn setup_new_probe(
        &mut self,
        cgroup: &util_cgroups::Cgroup,
        metrics: &Metrics,
    ) -> Option<crate::common::cgroup_events::ProbeSetup> {
        // if this is a pod, gets its uid, otherwise ignore this cgroup
        let pod_uid = super::pods::extract_pod_uid_from_cgroup(cgroup.fs_path())?;

        // get pod infos and add them as attributes to every measurement produced by the source
        let pod_infos = self
            .k8s_pods
            .get(&pod_uid)
            .inspect_err(|e| log::error!("failed to get K8S pod infos for pod {pod_uid}: {e:#}"))
            .ok()??;

        let attrs = vec![
            ("uid".to_string(), AttributeValue::String(pod_uid)),
            ("name".to_string(), AttributeValue::String(pod_infos.name)),
            ("namespace".to_string(), AttributeValue::String(pod_infos.namespace)),
            ("node".to_string(), AttributeValue::String(pod_infos.node)),
        ];
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);

        // setup the trigger according to the plugin's config
        let trigger = self.trigger.clone();

        // use the cgroup's "file stem" as the source name (it contains the pod uid)
        let name = cgroup.fs_path().file_stem().unwrap().to_str().unwrap().to_string();

        // ready!
        let source_settings = SourceSettings { name, trigger };
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}
