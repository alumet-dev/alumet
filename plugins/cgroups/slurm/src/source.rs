use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::attr::{JOB_REGEX_SLURM1, JOB_REGEX_SLURM2, JOB_STEP_REGEX, JobTagger, find_jobid_in_attrs};
use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    metrics::{AugmentedMetrics, Metrics},
    regex::RegexAttributesExtrator,
};

#[derive(Clone)]
pub struct JobSourceSetup {
    tagger: JobTagger,
    trigger: TriggerSpec,
    jobs_only: bool,
}

impl JobSourceSetup {
    pub fn new(config: super::Config) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);

        Ok(Self {
            tagger: JobTagger::new()?,
            trigger,
            jobs_only: config.jobs_only,
        })
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id", "job_step" and "user_id"
        let attrs = self.tagger.attributes_for_cgroup(cgroup);

        let is_job = !attrs.is_empty();
        let name = if is_job {
            // give a nice name to the source
            let job_id = find_jobid_in_attrs(&attrs).expect("job_id should be set");
            format!("slurm-job-{}", job_id)
        } else {
            // not a job, just a cgroup (for ex. a systemd service)
            if self.jobs_only {
                return None; // don't measure this cgroup
            }
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
