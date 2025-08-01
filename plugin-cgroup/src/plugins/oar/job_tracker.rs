use std::sync::{Arc, Mutex};

use anyhow::Context;
use rustc_hash::FxHashSet;

use crate::{
    common::{cgroup_events::CgroupRemovalCallback, regex::RegexAttributesExtrator},
    plugins::oar::{
        attr::{find_jobid_in_attrs, JOB_REGEX_OAR2, JOB_REGEX_OAR3},
        config::OarVersion,
    },
};

/// Tracks the jobs that are currently running on the node.
///
/// `JobTracker` is `Clone`, `Send` and `Sync`: you can clone it and pass it around freely.
#[derive(Clone)]
pub struct JobTracker {
    jobs: Arc<Mutex<FxHashSet<u64>>>,
}

/// Removes jobs from the [`JobTracker`] when the corresponding cgroup is deleted.
#[derive(Clone)]
pub struct JobCleaner {
    tracker: JobTracker,
    attr_extractor: RegexAttributesExtrator,
}

impl JobTracker {
    /// Creates a new, empty job tracker.
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(FxHashSet::default())),
        }
    }

    pub fn add(&mut self, job_id: u64) {
        self.jobs.lock().unwrap().insert(job_id);
    }

    pub fn add_multiple(&mut self, job_ids: impl Iterator<Item = u64>) {
        self.jobs.lock().unwrap().extend(job_ids);
    }

    pub fn remove(&mut self, job_id: u64) {
        self.jobs.lock().unwrap().remove(&job_id);
    }

    pub fn remove_multiple(&mut self, job_ids: impl Iterator<Item = u64>) {
        let mut j = self.jobs.lock().unwrap();
        for job in job_ids {
            j.remove(&job);
        }
    }

    pub fn known_jobs_sorted(&self) -> Vec<u64> {
        let mut v: Vec<u64> = {
            let v = self.jobs.lock().unwrap();
            v.iter().cloned().collect()
        };
        v.sort();
        v
    }
}

impl JobCleaner {
    pub fn with_version(tracker: &JobTracker, version: OarVersion) -> anyhow::Result<Self> {
        let attr_extractor = match version {
            OarVersion::Oar2 => RegexAttributesExtrator::new(JOB_REGEX_OAR2),
            OarVersion::Oar3 => RegexAttributesExtrator::new(JOB_REGEX_OAR3),
        }?;
        Ok(Self {
            tracker: tracker.clone(),
            attr_extractor,
        })
    }
}

impl CgroupRemovalCallback for JobCleaner {
    fn on_cgroups_removed(&mut self, cgroups: Vec<util_cgroups::Cgroup>) -> anyhow::Result<()> {
        let mut job_ids = Vec::new();
        for cgroup in cgroups {
            // If the regex matches, the cgroup corresponds to a job, and it should have a job id.
            let attrs = self
                .attr_extractor
                .extract(cgroup.canonical_path())
                .context("bad regex")?;
            if !attrs.is_empty() {
                let job_id = find_jobid_in_attrs(&attrs).context("if the regex matches, job_id should be set")?;
                job_ids.push(job_id);
            }
        }
        self.tracker.remove_multiple(job_ids.into_iter());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_send_sync() {
        fn f<T: Send + Sync>() {}
        // this compiles only if JobTracker is Send and Sync
        f::<JobTracker>();
    }
}
