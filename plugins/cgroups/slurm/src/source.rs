use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::{
    JobMonitoringLevel,
    attr::{SlurmJobTagger, find_jobid_in_attrs, find_key_in_attrs},
};
use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    job_annotation_transform::JobTagger,
    metrics::{AugmentedMetrics, Metrics},
};

#[derive(Clone)]
pub struct JobSourceSetup {
    tagger: SlurmJobTagger,
    trigger: TriggerSpec,
    ignore_non_jobs: bool,
    jobs_monitoring_level: JobMonitoringLevel,
}

impl JobSourceSetup {
    pub fn new(config: super::Config, tagger: SlurmJobTagger) -> anyhow::Result<Self> {
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
        let attrs = self.tagger.attributes_for_cgroup(cgroup);

        let job_id = find_jobid_in_attrs(&attrs);
        let step_id = find_key_in_attrs("step", &attrs);
        let sub_step = find_key_in_attrs("sub_step", &attrs);
        let task_id = find_key_in_attrs("task", &attrs);

        let name = if let Some(job_id) = job_id {
            let actual_level = if task_id.is_some() {
                JobMonitoringLevel::Task
            } else if sub_step.is_some() {
                JobMonitoringLevel::SubStep
            } else if step_id.is_some() {
                JobMonitoringLevel::Step
            } else {
                JobMonitoringLevel::Job
            };

            if actual_level > self.jobs_monitoring_level {
                return None;
            }

            match actual_level {
                JobMonitoringLevel::Job => format!("{}", job_id),
                JobMonitoringLevel::Step => step_id.unwrap().to_string(),
                JobMonitoringLevel::SubStep => format!("{}.{}", step_id.unwrap(), sub_step.unwrap()),
                JobMonitoringLevel::Task => format!("{}.{}.{}", step_id.unwrap(), sub_step.unwrap(), task_id.unwrap()),
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
