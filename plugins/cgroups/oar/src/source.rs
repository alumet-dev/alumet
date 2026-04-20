use std::process::Command;

use alumet::{measurement::AttributeValue, pipeline::elements::source::trigger::TriggerSpec};
use anyhow::{Context, anyhow};
use util_cgroups::Cgroup;

use crate::{
    Config, OarVersion,
    attr::{OarJobTagger, find_jobid_in_attrs, find_userid_in_attrs},
    job_tracker::JobTracker,
};
use util_cgroups_plugins::{
    cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
    job_annotation_transform::JobTagger,
    metrics::{AugmentedMetrics, Metrics},
};

#[derive(Clone)]
pub struct JobSourceSetup {
    tagger: OarJobTagger,
    username_from_userid: bool,
    trigger: TriggerSpec,
    tracker: JobTracker,
    jobs_only: bool,
}

impl JobSourceSetup {
    pub fn new(config: Config, tracker: JobTracker, tagger: OarJobTagger) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);
        match config.oar_version {
            OarVersion::Oar2 => Ok(Self {
                tagger,
                username_from_userid: false,
                trigger,
                tracker,
                jobs_only: config.jobs_only,
            }),
            OarVersion::Oar3 => Ok(Self {
                tagger,
                username_from_userid: true,
                trigger,
                tracker,
                jobs_only: config.jobs_only,
            }),
        }
    }
}

fn setup_probe_params<F>(
    mut attrs: Vec<(String, AttributeValue)>,
    username_from_userid: bool,
    jobs_only: bool,
    cgroup_name: &str,
    tracker: &mut JobTracker,
    resolve_username: F,
) -> Option<(String, Vec<(String, AttributeValue)>)>
where
    F: Fn(u64) -> anyhow::Result<String>,
{
    let is_job = !attrs.is_empty();

    if is_job {
        if username_from_userid {
            // Generate attribute "user" from "user_id".
            let user_id = find_userid_in_attrs(&attrs).expect("user_id should exist if username_from_userid is set");
            let user = resolve_username(user_id).expect("username resolution failed");
            attrs.push((String::from("user"), AttributeValue::String(user)));
        }

        // Add to job tracker
        let job_id = find_jobid_in_attrs(&attrs).expect("job_id should be set");
        tracker.add(job_id);

        // Give a nice name
        let name = format!("oar-job-{job_id}");
        Some((name, attrs))
    } else {
        // not a job, just a cgroup (for ex. a systemd service)
        if jobs_only {
            return None; // don't measure this cgroup
        }

        let name = format!("cgroup {cgroup_name}");
        Some((name, attrs))
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id" and ("user" or "user_id")
        let attrs = self.tagger.attributes_for_cgroup(cgroup);
        let cgroup_name = cgroup.unique_name();

        let (name, attrs) = setup_probe_params(
            attrs,
            self.username_from_userid,
            self.jobs_only,
            &cgroup_name,
            &mut self.tracker,
            username_from_id,
        )?;

        let trigger = self.trigger.clone();
        let source_settings = SourceSettings { name, trigger };
        let metrics = AugmentedMetrics::with_common_attr_vec(metrics, attrs);
        Some(ProbeSetup {
            metrics,
            source_settings,
        })
    }
}

fn username_params<F>(id: u64, params: F) -> anyhow::Result<String>
where
    F: Fn(u64) -> anyhow::Result<String>,
{
    params(id)
}

fn username_from_id(id: u64) -> anyhow::Result<String> {
    username_params(id, |id| {
        let child = Command::new("id")
            .args(&["-n", "-u", &id.to_string()])
            .spawn()
            .context("failed to spawn process id")?;

        let output = child
            .wait_with_output()
            .context("failed to wait for process id to terminate")?;

        if !output.status.success() {
            let error_message = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(anyhow!("process id failed with {}", output.status).context(error_message));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOCK_CGROUP: &str = "oar.slice";
    const MOCK_ID: u64 = 10;

    #[test]
    fn test_setup_probe_params_with_job() {
        let mut tracker = JobTracker::new();
        let attrs = vec![
            ("job_id".into(), AttributeValue::U64(MOCK_ID)),
            ("user_id".into(), AttributeValue::U64(MOCK_ID)),
        ];

        let mock = |id| Ok(format!("user_{id}"));
        let result = setup_probe_params(attrs, true, false, MOCK_CGROUP, &mut tracker, mock);
        let (name, attrs) = result.unwrap();

        assert_eq!(name, format!("oar-job-{MOCK_ID}"));
        assert!(attrs.iter().any(|(k, _)| k == "user"));
    }

    #[test]
    fn test_setup_probe_params_jobs_only_filters_cgroup() {
        let mut tracker = JobTracker::new();
        let attrs = vec![];
        let mock = |_id| Ok(String::from("user"));

        let result = setup_probe_params(attrs, false, true, MOCK_CGROUP, &mut tracker, mock);
        assert!(result.is_none());
    }

    #[test]
    fn test_setup_probe_params_is_tracked() {
        let mut tracker = JobTracker::new();
        let attrs = vec![];
        let mock = |_id| Ok(String::from("user"));

        let result = setup_probe_params(attrs, false, false, MOCK_CGROUP, &mut tracker, mock);

        let (name, _) = result.unwrap();
        assert_eq!(name, format!("cgroup {MOCK_CGROUP}"));
    }

    #[test]
    fn test_username_from_id_with_invalid_value() {
        let result = username_from_id(999_999_999);
        assert!(result.is_err());
    }

    #[test]
    fn test_username_params_ok() {
        let mock = |id| Ok(format!("user_{id}"));
        let result = username_params(MOCK_ID, mock).unwrap();
        assert_eq!(result, format!("user_{MOCK_ID}"));
    }

    #[test]
    fn test_username_params_fail() {
        let mock = |_id| Err(anyhow!("fail"));
        let result = username_params(MOCK_ID, mock);
        assert!(result.is_err());
    }
}
