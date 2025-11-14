use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::{
    JobMonitoringLevel,
    attr::{JobTagger, find_jobid_in_attrs, find_key_in_attrs},
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
        let attrs = self.tagger.attributes_for_cgroup(cgroup);

        let job_id = find_jobid_in_attrs(&attrs);
        let step_id = find_key_in_attrs("step", &attrs);
        let sub_step = find_key_in_attrs("sub_step", &attrs);
        let task_id = find_key_in_attrs("task", &attrs);

        let name = if let Some(job_id) = job_id {
            match self.jobs_monitoring_level {
                JobMonitoringLevel::Job => {
                    if step_id.is_some() || sub_step.is_some() || task_id.is_some() {
                        return None;
                    } else {
                        // it's job level
                        format!("{}", job_id)
                    }
                }
                JobMonitoringLevel::Step => {
                    if sub_step.is_some() || task_id.is_some() {
                        return None;
                    } else if step_id.is_none() {
                        // it's job level
                        format!("{}", job_id)
                    } else {
                        // it's step level
                        step_id.unwrap().to_string()
                    }
                }
                JobMonitoringLevel::SubStep => {
                    if task_id.is_some() {
                        return None;
                    } else if step_id.is_none() && sub_step.is_none() {
                        // it's job level
                        format!("{}", job_id)
                    } else if sub_step.is_none() {
                        // it's step level
                        step_id.unwrap().to_string()
                    } else {
                        // it's sub step level
                        format!("{}.{}", step_id.unwrap(), sub_step.unwrap())
                    }
                }
                JobMonitoringLevel::Task => {
                    if task_id.is_some() {
                        // it's task level
                        format!("{}.{}.{}", step_id.unwrap(), sub_step.unwrap(), task_id.unwrap())
                    } else if step_id.is_none() && sub_step.is_none() && task_id.is_none() {
                        // it's job level
                        format!("{}", job_id)
                    } else if sub_step.is_none() && task_id.is_none() {
                        // it's step level
                        step_id.unwrap().to_string()
                    } else if task_id.is_none() {
                        //it's substep level
                        format!("{}.{}", step_id.unwrap(), sub_step.unwrap())
                    } else {
                        return None;
                    }
                }
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
