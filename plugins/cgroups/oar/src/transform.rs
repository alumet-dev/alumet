use std::cell::LazyCell;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer},
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
};

use crate::job_tracker::JobTracker;

/// Add the list of current jobs to every measurement that is not job-specific.
/// This is used to relate the measurements to the jobs, for searching, making dashboards, etc.
pub struct JobInfoAttacher {
    tracker: JobTracker,
}

impl JobInfoAttacher {
    pub fn new(tracker: JobTracker) -> Self {
        Self { tracker }
    }
}

impl Transform for JobInfoAttacher {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // lazily initialized
        let current_job_list = LazyCell::new(|| self.tracker.known_jobs_sorted().into_iter().collect::<Vec<_>>());
        for m in measurements.iter_mut() {
            if !m.attributes_keys().any(|k| k == "job_id") {
                // This measurement is not job-specific, attach the list of running jobs.
                // See issue #209.
                let jobs_attr = current_job_list.clone();
                m.add_attr("involved_jobs", AttributeValue::ListU64(jobs_attr));
            }
        }
        Ok(())
    }
}
