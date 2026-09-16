use alumet::pipeline::elements::source::trigger::TriggerSpec;

use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    job_annotation_transform::JobTagger,
    metrics::{AugmentedMetrics, Metrics},
};

use crate::containers::AutoContainerRegistry;

/// Setup for container cgroup probes
#[derive(Clone)]
pub struct SourceSetup {
    pub trigger: TriggerSpec,
    pub container_registry: AutoContainerRegistry,
}

impl CgroupSetupCallback for SourceSetup {
    fn setup_new_probe(
        &mut self,
        cgroup: &util_cgroups::Cgroup,
        metrics: &Metrics,
    ) -> Option<util_cgroups_plugins::cgroup_events::ProbeSetup> {
        // Retrieves associated attributes
        let attrs = self.container_registry.attributes_for_cgroup(cgroup);

        if attrs.is_empty() {
            // If empty, this is NOT a container
            return None;
        }

        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);

        // Setup the trigger according to the plugin's config
        let trigger = self.trigger.clone();

        // Use the container ID from the path as the source name
        let name = cgroup.fs_path().file_stem().unwrap().to_str().unwrap().to_string();

        // Ready!
        let source_settings = SourceSettings { name, trigger };
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}
