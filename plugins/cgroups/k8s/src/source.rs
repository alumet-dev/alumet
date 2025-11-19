use alumet::pipeline::elements::source::trigger::TriggerSpec;

use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    job_annotation_transform::JobTagger,
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
    ) -> Option<util_cgroups_plugins::cgroup_events::ProbeSetup> {
        // if this is a pod, gets its uid, otherwise ignore this cgroup
        let attrs = self.k8s_pods.attributes_for_cgroup(cgroup);

        if attrs.is_empty() {
            // We don't want to monitor if empty
            return None;
        }

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
