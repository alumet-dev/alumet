use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::{
    Config, JobMonitoringLevel,
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
    pub fn new(config: Config, tagger: SlurmJobTagger) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);

        Ok(Self {
            tagger,
            trigger,
            ignore_non_jobs: config.ignore_non_jobs,
            jobs_monitoring_level: config.jobs_monitoring_level,
        })
    }
}

fn setup_name(
    max_level: JobMonitoringLevel,
    job_id: Option<u64>,
    step_id: Option<&str>,
    sub_step: Option<&str>,
    task_id: Option<&str>,
) -> Option<String> {
    let job_id = job_id?;

    let actual_level = if task_id.is_some() {
        JobMonitoringLevel::Task
    } else if sub_step.is_some() {
        JobMonitoringLevel::SubStep
    } else if step_id.is_some() {
        JobMonitoringLevel::Step
    } else {
        JobMonitoringLevel::Job
    };

    if actual_level > max_level {
        return None;
    }

    Some(match actual_level {
        JobMonitoringLevel::Job => job_id.to_string(),
        JobMonitoringLevel::Step => step_id.unwrap().to_string(),
        JobMonitoringLevel::SubStep => format!("{}.{}", step_id.unwrap(), sub_step.unwrap()),
        JobMonitoringLevel::Task => format!("{}.{}.{}", step_id.unwrap(), sub_step.unwrap(), task_id.unwrap()),
    })
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id", "job_step" and "user_id"
        let attrs = self.tagger.attributes_for_cgroup(cgroup);

        let job_id = find_jobid_in_attrs(&attrs);
        let step_id = find_key_in_attrs("step", &attrs);
        let sub_step = find_key_in_attrs("sub_step", &attrs);
        let task_id = find_key_in_attrs("task", &attrs);

        let name = match setup_name(
            self.jobs_monitoring_level.clone(),
            job_id,
            step_id.as_deref(),
            sub_step.as_deref(),
            task_id.as_deref(),
        ) {
            Some(name) => name,
            None if job_id.is_none() && self.ignore_non_jobs => return None,
            None if job_id.is_none() => format!("cgroup {}", cgroup.unique_name()),
            None => return None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_name_job_ok() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Job, Some(54), None, None, None),
            Some("54".into())
        );
    }

    #[test]
    fn test_setup_name_job_reject_step() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Job, Some(54), Some("1"), None, None),
            None
        );
    }

    #[test]
    fn test_setup_name_step_ok() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Step, Some(54), Some("1"), None, None),
            Some("1".into())
        );
    }

    #[test]
    fn test_setup_name_step_fallback_job() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Step, Some(54), None, None, None),
            Some("54".into())
        );
    }

    #[test]
    fn test_setup_name_step_reject_substep() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Step, Some(54), Some("1"), Some("2"), None),
            None
        );
    }

    #[test]
    fn test_setup_name_substep_job() {
        assert_eq!(
            setup_name(JobMonitoringLevel::SubStep, Some(54), None, None, None),
            Some("54".into())
        );
    }

    #[test]
    fn test_setup_name_substep_step() {
        assert_eq!(
            setup_name(JobMonitoringLevel::SubStep, Some(54), Some("1"), None, None),
            Some("1".into())
        );
    }

    #[test]
    fn test_setup_name_substep_ok() {
        assert_eq!(
            setup_name(JobMonitoringLevel::SubStep, Some(54), Some("1"), Some("2"), None),
            Some("1.2".into())
        );
    }

    #[test]
    fn test_setup_name_substep_reject_task() {
        assert_eq!(
            setup_name(JobMonitoringLevel::SubStep, Some(54), Some("1"), Some("2"), Some("3")),
            None
        );
    }

    #[test]
    fn test_setup_name_task_ok() {
        assert_eq!(
            setup_name(JobMonitoringLevel::Task, Some(54), Some("1"), Some("2"), Some("3")),
            Some("1.2.3".into())
        );
    }
}
