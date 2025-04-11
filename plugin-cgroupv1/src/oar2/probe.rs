use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    metrics::TypedMetricId,
    pipeline::{elements::error::PollError, Source},
};
use std::result::Result::Ok;

use super::utils::Cgroupv1Resources;

use crate::cgroupv1::gather_value;

#[derive(Debug)]
pub struct OarJobSource {
    pub cpu_metric: TypedMetricId<u64>,
    pub memory_metric: TypedMetricId<u64>,
    pub cgroup_v1_metric_file: Cgroupv1Resources,
}

impl Source for OarJobSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let point_to_push = gather_value(self, timestamp)?;
        for point in point_to_push {
            measurements.push(point);
        }
        Ok(())
    }
}
