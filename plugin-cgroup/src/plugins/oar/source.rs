use std::process::Command;

use alumet::{measurement::AttributeValue, pipeline::elements::source::trigger::TriggerSpec};
use anyhow::{Context, anyhow};
use util_cgroups::Cgroup;

use super::OarVersion;
use crate::{
    common::{
        cgroup_events::{CgroupSetupCallback, ProbeSetup, SourceSettings},
        metrics::{AugmentedMetrics, Metrics},
        regex::RegexAttributesExtrator,
    },
    plugins::oar::{
        attr::{JOB_REGEX_OAR2, JOB_REGEX_OAR3, find_jobid_in_attrs, find_userid_in_attrs},
        job_tracker::JobTracker,
    },
};

#[derive(Clone)]
pub struct JobSourceSetup {
    extractor: RegexAttributesExtrator,
    username_from_userid: bool,
    trigger: TriggerSpec,
    tracker: JobTracker,
    jobs_only: bool,
}

impl JobSourceSetup {
    pub fn new(config: super::Config, tracker: JobTracker) -> anyhow::Result<Self> {
        let trigger = TriggerSpec::at_interval(config.poll_interval);
        match config.oar_version {
            OarVersion::Oar2 => Ok(Self {
                extractor: RegexAttributesExtrator::new(JOB_REGEX_OAR2)?,
                username_from_userid: false,
                trigger,
                tracker,
                jobs_only: config.jobs_only,
            }),
            OarVersion::Oar3 => Ok(Self {
                extractor: RegexAttributesExtrator::new(JOB_REGEX_OAR3)?,
                username_from_userid: true,
                trigger,
                tracker,
                jobs_only: config.jobs_only,
            }),
        }
    }
}

impl CgroupSetupCallback for JobSourceSetup {
    fn setup_new_probe(&mut self, cgroup: &Cgroup, metrics: &Metrics) -> Option<ProbeSetup> {
        // extracts attributes "job_id" and ("user" or "user_id")
        let mut attrs = self
            .extractor
            .extract(cgroup.canonical_path())
            .expect("bad regex: it should only match if the input can be parsed into the specified types");

        let is_job = !attrs.is_empty();
        let name: String;

        if is_job {
            if self.username_from_userid {
                // Generate attribute "user" from "user_id".
                let user_id =
                    find_userid_in_attrs(&attrs).expect("user_id should exist if username_from_userid is set");
                let user = username_from_id(user_id).unwrap(); // TODO handle error here
                attrs.push((String::from("user"), AttributeValue::String(user)));
            }

            // add to job tracker
            let job_id = find_jobid_in_attrs(&attrs).expect("job_id should be set");
            self.tracker.add(job_id);

            // give a nice name
            name = format!(
                "oar-job-{}",
                find_jobid_in_attrs(&attrs).expect("job_id should always be set")
            );
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

fn username_from_id(id: u64) -> anyhow::Result<String> {
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
    let username = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(username)
}

#[cfg(test)]
mod tests {
    // use super::*;

    // #[test]
    // fn test_username_from_id() {
    //     let username = username_from_id(1000).unwrap();
    //     println!("{username}");
    // }
}
