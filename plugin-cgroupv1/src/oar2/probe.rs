use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    pipeline::{elements::error::PollError, Source},
};
use std::result::Result::Ok;

use crate::cgroupv1::{Cgroupv1Probe, Metrics};

pub struct Oar2Probe {
    cgroupv1: Cgroupv1Probe,
    additional_attrs: Vec<(String, AttributeValue)>,
}

impl Oar2Probe {
    pub fn new(
        job_id: String,
        metrics: Metrics,
        cpuacct_usage_filepath: Option<String>,
        memory_usage_filepath: Option<String>,
    ) -> Result<Self, anyhow::Error> {
        let additional_attrs = vec![("job_id".to_string(), AttributeValue::String(job_id))];
        Ok(Self {
            cgroupv1: Cgroupv1Probe::new(metrics, cpuacct_usage_filepath, memory_usage_filepath)?,
            additional_attrs,
        })
    }

    pub fn collect_measurements(&mut self, timestamp: Timestamp) -> Result<Vec<MeasurementPoint>, PollError> {
        self.cgroupv1.collect_measurements(timestamp, &self.additional_attrs)
    }
}

impl Source for Oar2Probe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for point in self.collect_measurements(timestamp)? {
            measurements.push(point);
        }
        Ok(())
    }
}
