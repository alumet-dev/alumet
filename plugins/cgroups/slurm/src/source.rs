use alumet::{measurement::AttributeValue, pipeline::elements::source::trigger::TriggerSpec};
use itertools::Itertools;
use util_cgroups::Cgroup;

use crate::{
    Config,
    attr::{SlurmJobTagger, find_jobid_in_attrs, find_key_in_attrs},
    config::JobMonitoringLevel,
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

struct JobAttrs {
    job_id: Option<u64>,
    step_id: Option<String>,
    substep_id: Option<String>,
    task_id: Option<String>,
}

impl JobAttrs {
    fn from(attrs: &[(String, AttributeValue)]) -> Self {
        let job_id = find_jobid_in_attrs(&attrs);
        let step_id = find_key_in_attrs("step_id", &attrs);
        let substep_id = find_key_in_attrs("substep_id", &attrs);
        let task_id = find_key_in_attrs("task_id", &attrs);
        Self {
            job_id,
            step_id,
            substep_id,
            task_id,
        }
    }

    fn job_level(&self) -> Option<JobMonitoringLevel> {
        if self.job_id.is_none() {
            return None;
        }

        let lvl = if self.task_id.is_some() {
            JobMonitoringLevel::Task
        } else if self.substep_id.is_some() {
            JobMonitoringLevel::SubStep
        } else if self.step_id.is_some() {
            JobMonitoringLevel::Step
        } else {
            JobMonitoringLevel::Job
        };
        Some(lvl)
    }
}

fn source_name(attrs: &JobAttrs, cgroup: &Cgroup) -> String {
    match attrs.job_id {
        None => format!("cgroup {}", cgroup.unique_name()),
        Some(job_id) => {
            // With psslurm, it's possible to have a "task" level directly in a job, so be careful.
            // Consider each part independently, and join them like "a.b.c"
            let job_id = job_id.to_string();
            let parts = [&Some(job_id), &attrs.step_id, &attrs.substep_id, &attrs.task_id];
            parts.into_iter().flatten().join(".")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SourcePreparation {
    name: String,
    attrs: Vec<(String, AttributeValue)>,
}

impl JobSourceSetup {
    fn prepare(&mut self, cgroup: &Cgroup) -> Option<SourcePreparation> {
        // extracts attributes like "job_id" etc.
        let attrs_vec = self.tagger.attributes_for_cgroup(cgroup);
        let attrs_struct = JobAttrs::from(&attrs_vec);

        // filter out the cgroups we don't want
        match attrs_struct.job_level() {
            Some(job_level) if job_level > self.jobs_monitoring_level => return None,
            None if self.ignore_non_jobs => return None,
            _ => (),
        }

        // give a nice name to the alumet source
        let name = source_name(&attrs_struct, cgroup);
        Some(SourcePreparation { name, attrs: attrs_vec })
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        let SourcePreparation { name, attrs } = self.prepare(cgroup)?;
        let settings = SourceSettings {
            name,
            trigger: self.trigger.clone(),
        };
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);
        Some(ProbeSetup {
            metrics,
            source_settings: settings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{JobAttrs, JobMonitoringLevel, JobSourceSetup};
    use crate::{
        attr::{JOB_REGEX_PSSLURM, JOB_REGEX_SLURM2, JobCgroupRegexExtractorStrategy, SlurmJobTagger},
        source::SourcePreparation,
    };
    use alumet::{measurement::AttributeValue, pipeline::elements::source::trigger::TriggerSpec};
    use std::{path::PathBuf, time::Duration};
    use util_cgroups::{Cgroup, CgroupHierarchy, CgroupVersion};
    use util_cgroups_plugins::regex::RegexAttributesExtrator;

    #[test]
    fn job_level_from_attrs() {
        let attrs = JobAttrs {
            job_id: None,
            step_id: None,
            substep_id: None,
            task_id: None,
        };
        assert_eq!(attrs.job_level(), None);

        let attrs = JobAttrs {
            job_id: Some(123),
            step_id: None,
            substep_id: None,
            task_id: None,
        };
        assert_eq!(attrs.job_level(), Some(JobMonitoringLevel::Job));

        let attrs = JobAttrs {
            job_id: Some(1),
            step_id: Some(String::from("2")),
            substep_id: None,
            task_id: None,
        };
        assert_eq!(attrs.job_level(), Some(JobMonitoringLevel::Step));

        let attrs = JobAttrs {
            job_id: Some(1),
            step_id: Some(String::from("2")),
            substep_id: Some(String::from("3")),
            task_id: None,
        };
        assert_eq!(attrs.job_level(), Some(JobMonitoringLevel::SubStep));

        let attrs = JobAttrs {
            job_id: Some(1),
            step_id: Some(String::from("2")),
            substep_id: Some(String::from("3")),
            task_id: Some(String::from("4")),
        };
        assert_eq!(attrs.job_level(), Some(JobMonitoringLevel::Task));

        let attrs = JobAttrs {
            job_id: Some(1),
            step_id: None,
            substep_id: None,
            task_id: Some(String::from("tasks")),
        };
        assert_eq!(attrs.job_level(), Some(JobMonitoringLevel::Task));
    }

    #[test]
    fn source_setup_for_psslurm_cgroupv2() {
        // create fake cgroup hierarchy (they don't even exist on disk, it's only our memory representation of cgroups that is created)
        let h = CgroupHierarchy::manually_unchecked(
            PathBuf::from("/sys/fs/cgroup"),
            CgroupVersion::V2,
            vec![String::from("cpuset")],
        );

        // create setup helper; level=job
        let tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_PSSLURM).unwrap(),
        ));
        let trigger_spec = TriggerSpec::at_interval(Duration::from_secs(1));
        let mut jss = JobSourceSetup {
            tagger,
            trigger: trigger_spec,
            ignore_non_jobs: true,
            jobs_monitoring_level: JobMonitoringLevel::Job,
        };

        // setup and check on various cgroups
        let job = Cgroup::from_cgroup_path(&h, "/slurm/user-testu/job-123".to_string());
        let task = Cgroup::from_cgroup_path(&h, "/slurm/user-testu/job-123/tasks".to_string());
        let step = Cgroup::from_cgroup_path(&h, "/slurm/user-testu/job-123/step-0".to_string());
        let task_in_step = Cgroup::from_cgroup_path(&h, "/slurm/user-testu/job-123/step-0/tasks".to_string());
        let other = Cgroup::from_cgroup_path(&h, "/other/not/a/job".to_string());

        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![
                    (String::from("user"), AttributeValue::String(String::from("testu"))),
                    (String::from("job_id"), AttributeValue::U64(123)),
                ]
            })
        );
        assert_eq!(jss.prepare(&task), None);
        assert_eq!(jss.prepare(&step), None);
        assert_eq!(jss.prepare(&task_in_step), None);
        assert_eq!(jss.prepare(&other), None);

        // change: level=task
        jss.jobs_monitoring_level = JobMonitoringLevel::Task;
        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![
                    (String::from("user"), AttributeValue::String(String::from("testu"))),
                    (String::from("job_id"), AttributeValue::U64(123)),
                ]
            })
        );
        assert_eq!(
            jss.prepare(&task),
            Some(SourcePreparation {
                name: String::from("123.tasks"),
                attrs: vec![
                    (String::from("user"), AttributeValue::String(String::from("testu"))),
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("task_id"), AttributeValue::String(String::from("tasks"))),
                ]
            })
        );
        assert_eq!(
            jss.prepare(&step),
            Some(SourcePreparation {
                name: String::from("123.0"),
                attrs: vec![
                    (String::from("user"), AttributeValue::String(String::from("testu"))),
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0"))),
                ]
            })
        );
        assert_eq!(
            jss.prepare(&task_in_step),
            Some(SourcePreparation {
                name: String::from("123.0.tasks"),
                attrs: vec![
                    (String::from("user"), AttributeValue::String(String::from("testu"))),
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0"))),
                    (String::from("task_id"), AttributeValue::String(String::from("tasks"))),
                ]
            })
        );
        assert_eq!(jss.prepare(&other), None);

        // change: accept non-jobs
        jss.ignore_non_jobs = false;
        assert_eq!(
            jss.prepare(&other),
            Some(SourcePreparation {
                name: String::from("cgroup /other/not/a/job"),
                attrs: vec![]
            })
        );
    }

    #[test]
    fn source_setup_for_slurm_cgroupv2() {
        // create fake cgroup hierarchy (they don't even exist on disk, it's only our memory representation of cgroups that is created)
        let h = CgroupHierarchy::manually_unchecked(
            PathBuf::from("/sys/fs/cgroup"),
            CgroupVersion::V2,
            vec![String::from("cpuset")],
        );

        // create setup helper; level=job
        let tagger = SlurmJobTagger::new(JobCgroupRegexExtractorStrategy::Fixed(
            RegexAttributesExtrator::new(JOB_REGEX_SLURM2).unwrap(),
        ));
        let trigger_spec = TriggerSpec::at_interval(Duration::from_secs(1));
        let mut jss = JobSourceSetup {
            tagger,
            trigger: trigger_spec,
            ignore_non_jobs: true,
            jobs_monitoring_level: JobMonitoringLevel::Job,
        };

        // setup and check on various cgroups
        let job = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_123".to_string());
        let step = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_123/step_0".to_string());
        let substep = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_123/step_0/mysubstep".to_string());
        let task = Cgroup::from_cgroup_path(&h, "/slurmstepd.scope/job_123/step_0/mysubstep/task_99".to_string());
        let other = Cgroup::from_cgroup_path(&h, "/other/not/a/job".to_string());

        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![(String::from("job_id"), AttributeValue::U64(123)),]
            })
        );
        assert_eq!(jss.prepare(&step), None);
        assert_eq!(jss.prepare(&substep), None);
        assert_eq!(jss.prepare(&task), None);
        assert_eq!(jss.prepare(&other), None);

        // change: level=step
        jss.jobs_monitoring_level = JobMonitoringLevel::Step;
        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![(String::from("job_id"), AttributeValue::U64(123)),]
            })
        );
        assert_eq!(
            jss.prepare(&step),
            Some(SourcePreparation {
                name: String::from("123.0"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0")))
                ]
            })
        );
        assert_eq!(jss.prepare(&substep), None);
        assert_eq!(jss.prepare(&task), None);
        assert_eq!(jss.prepare(&other), None);

        // change: level=substep
        jss.jobs_monitoring_level = JobMonitoringLevel::SubStep;
        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![(String::from("job_id"), AttributeValue::U64(123)),]
            })
        );
        assert_eq!(
            jss.prepare(&step),
            Some(SourcePreparation {
                name: String::from("123.0"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0")))
                ]
            })
        );
        assert_eq!(
            jss.prepare(&substep),
            Some(SourcePreparation {
                name: String::from("123.0.mysubstep"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0"))),
                    (
                        String::from("substep_id"),
                        AttributeValue::String(String::from("mysubstep"))
                    ),
                ]
            })
        );
        assert_eq!(jss.prepare(&task), None);
        assert_eq!(jss.prepare(&other), None);

        // change: level=task
        jss.jobs_monitoring_level = JobMonitoringLevel::Task;
        assert_eq!(
            jss.prepare(&job),
            Some(SourcePreparation {
                name: String::from("123"),
                attrs: vec![(String::from("job_id"), AttributeValue::U64(123)),]
            })
        );
        assert_eq!(
            jss.prepare(&step),
            Some(SourcePreparation {
                name: String::from("123.0"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0")))
                ]
            })
        );
        assert_eq!(
            jss.prepare(&substep),
            Some(SourcePreparation {
                name: String::from("123.0.mysubstep"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0"))),
                    (
                        String::from("substep_id"),
                        AttributeValue::String(String::from("mysubstep"))
                    ),
                ]
            })
        );
        assert_eq!(
            jss.prepare(&task),
            Some(SourcePreparation {
                name: String::from("123.0.mysubstep.99"),
                attrs: vec![
                    (String::from("job_id"), AttributeValue::U64(123)),
                    (String::from("step_id"), AttributeValue::String(String::from("0"))),
                    (
                        String::from("substep_id"),
                        AttributeValue::String(String::from("mysubstep"))
                    ),
                    (String::from("task_id"), AttributeValue::String(String::from("99"))),
                ]
            })
        );
        assert_eq!(jss.prepare(&other), None);
    }
}
