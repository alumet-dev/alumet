use alumet::pipeline::elements::source::trigger::TriggerSpec;

use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    metrics::{AugmentedMetrics, Metrics},
};

#[derive(Clone)]
pub struct SourceSetup {
    pub trigger: TriggerSpec,
}

impl CgroupSetupCallback for SourceSetup {
    fn setup_new_probe(
        &mut self,
        cgroup: &util_cgroups::Cgroup,
        metrics: &Metrics,
    ) -> Option<util_cgroups_plugins::cgroup_events::ProbeSetup> {
        // no custom attributes, this is the "raw" cgroup plugin :)
        let metrics = AugmentedMetrics::no_additional_attribute(metrics);

        // setup the trigger according to the plugin's config
        let trigger = self.trigger.clone();

        // use the cgroup name as the source name
        let name = cgroup.unique_name().to_string();

        // ok
        let source_settings = SourceSettings { name, trigger };
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}
