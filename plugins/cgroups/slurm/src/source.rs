use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::{
    JobMonitoringLevel,
    attr::{JobTagger, find_job_step_in_attrs, find_jobid_in_attrs, try_update_job_step_in_attrs},
};
use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    metrics::{AugmentedMetrics, Metrics},
};

#[derive(Clone)]
pub struct JobSourceSetup {
    tagger: JobTagger,
    trigger: TriggerSpec,
    ignore_non_jobs: bool,
    jobs_monitoring_level: JobMonitoringLevel,
}

impl JobSourceSetup {
    pub fn new(config: super::Config, tagger: JobTagger) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);

        Ok(Self {
            tagger,
            trigger,
            ignore_non_jobs: config.ignore_non_jobs,
            jobs_monitoring_level: config.jobs_monitoring_level,
        })
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id", "job_step" and "user_id"
        let mut attrs = self.tagger.attributes_for_cgroup(cgroup);

        let job_id = find_jobid_in_attrs(&attrs);

        let name = if let Some(job_id) = job_id {
            if let Some(step_id) = find_job_step_in_attrs(&attrs) {
                // This cgroup is a step/subtask related to the job
                if self.jobs_monitoring_level == JobMonitoringLevel::Step {
                    // We want to keep it
                    let tmp_name = format!("{}.{}", job_id, step_id);
                    try_update_job_step_in_attrs(&mut attrs, tmp_name.clone());
                    tmp_name
                } else {
                    // We don't want to monitor it
                    return None;
                }
            } else {
                // This cgroup is the main one for the job
                format!("{}", job_id)
            }
        } else if self.ignore_non_jobs {
            return None;
        } else {
            format!("cgroup {}", cgroup.unique_name())
        };

        let trigger = self.trigger.clone();
        let source_settings = SourceSettings { name, trigger };
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}
