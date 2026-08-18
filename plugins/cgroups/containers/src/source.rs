use alumet::pipeline::elements::source::trigger::TriggerSpec;

use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    job_annotation_transform::JobTagger,
    metrics::{AugmentedMetrics, Metrics},
};

use crate::registry::ContainerRegistry;

/// Setup for container cgroup probes
#[derive(Clone)]
pub struct SourceSetup {
    pub trigger: TriggerSpec,
    pub container_registry: ContainerRegistry,
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
        // Extract container ID from the cgroup path
        let name = extract_container_name_from_path(cgroup.fs_path()).unwrap_or_else(|| {
            cgroup.fs_path()
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown_container")
                .to_string()
        });

        // Ready!
        let source_settings = SourceSettings { name, trigger };
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}

/// Extracts a descriptive name from a container cgroup path
fn extract_container_name_from_path(path: &std::path::Path) -> Option<String> {
    use crate::extraction::extract_container_id;
    use crate::config::ContainerRuntime;
    
    // Try to extract container ID from the path
    // We'll use it as a fallback name
    if let Some(container_id) = extract_container_id(path, ContainerRuntime::Docker) {
        return Some(container_id);
    }
    
    if let Some(container_id) = extract_container_id(path, ContainerRuntime::Podman) {
        return Some(container_id);
    }
    
    // If that fails, use the last path component
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
}