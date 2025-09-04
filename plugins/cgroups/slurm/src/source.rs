use alumet::pipeline::elements::source::trigger::TriggerSpec;
use util_cgroups::Cgroup;

use crate::attr::{JOB_REGEX_SLURM1, JOB_REGEX_SLURM2, JOB_STEP_REGEX, find_jobid_in_attrs};
use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    metrics::{AugmentedMetrics, Metrics},
    regex::RegexAttributesExtrator,
};

#[derive(Clone)]
pub struct JobSourceSetup {
    extractor_v1: RegexAttributesExtrator,
    extractor_v2: RegexAttributesExtrator,
    step_extractor: RegexAttributesExtrator,
    trigger: TriggerSpec,
    jobs_only: bool,
}

impl JobSourceSetup {
    pub fn new(config: super::Config) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);

        Ok(Self {
            extractor_v1: RegexAttributesExtrator::new(JOB_REGEX_SLURM1)?,
            extractor_v2: RegexAttributesExtrator::new(JOB_REGEX_SLURM2)?,
            step_extractor: RegexAttributesExtrator::new(JOB_STEP_REGEX)?,
            trigger,
            jobs_only: config.jobs_only,
        })
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id" and ("user" or "user_id")
        let version = cgroup.hierarchy().version();
        let extractor = match version {
            util_cgroups::CgroupVersion::V1 => &mut self.extractor_v1,
            util_cgroups::CgroupVersion::V2 => &mut self.extractor_v2,
        };

        let mut attrs = extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types");

        let is_job = !attrs.is_empty();
        let name: String;

        if is_job {
            // give a nice name to the source
            let job_id = find_jobid_in_attrs(&attrs).expect("job_id should be set");
            name = format!("slurm-job-{}", job_id);

            // check if the cgroup is a job step and extract its name as a "job_step" attribute
            self.step_extractor
                .extract_into(cgroup.canonical_path(), &mut attrs)
                .expect("bad regex: it should only match if the input can be parsed into the specified types");
        } else {
            // not a job, just a cgroup (for ex. a systemd service)
            if self.jobs_only {
                return None; // don't measure this cgroup
            }
            name = format!("cgroup {}", cgroup.unique_name());
        }

        let trigger = self.trigger.clone();
        let source_settings = SourceSettings { name, trigger };
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}
