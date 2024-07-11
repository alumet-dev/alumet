use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::{CounterDiff, CounterDiffUpdate},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use crate::cgroupv2::{CgroupV2Metric, Metrics};

use super::utils::{gather_value, CgroupV2MetricFile};

pub struct CgroupV2prob {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

impl CgroupV2prob {
    pub fn new(
        metric: Metrics,
        metric_file: CgroupV2MetricFile,
        counter_tot: CounterDiff,
        counter_sys: CounterDiff,
        counter_usr: CounterDiff,
    ) -> anyhow::Result<CgroupV2prob> {
        return Ok(CgroupV2prob {
            cgroup_v2_metric_file: metric_file,
            time_tot: counter_tot,
            time_usr: counter_usr,
            time_sys: counter_sys,
            time_used_tot: metric.time_used_tot,
            time_used_system_mode: metric.time_used_system_mode,
            time_used_user_mode: metric.time_used_user_mode,
        });
    }
}

impl alumet::pipeline::Source for CgroupV2prob {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut file_buffer = String::new();
        let metrics: CgroupV2Metric = gather_value(&mut self.cgroup_v2_metric_file, &mut file_buffer)?;
        let diff_tot = match self.time_tot.update(metrics.time_used_tot) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) | CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_usr = match self.time_usr.update(metrics.time_used_user_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let diff_sys = match self.time_sys.update(metrics.time_used_system_mode) {
            CounterDiffUpdate::FirstTime => None,
            CounterDiffUpdate::Difference(diff) => Some(diff),
            CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
        };
        let consumer = ResourceConsumer::ControlGroup {
            path: (self.cgroup_v2_metric_file.path.to_string_lossy().to_string().into()),
        };
        if let Some(value_tot) = diff_tot {
            let p_tot: MeasurementPoint = MeasurementPoint::new(
                timestamp,
                self.time_used_tot,
                Resource::LocalMachine,
                consumer.clone(),
                value_tot as u64,
            )
            .with_attr("name", AttributeValue::String(metrics.name.clone()));
            measurements.push(p_tot);
        }
        if let Some(value_usr) = diff_usr {
            let p_usr: MeasurementPoint = MeasurementPoint::new(
                timestamp,
                self.time_used_user_mode,
                Resource::LocalMachine,
                consumer.clone(),
                value_usr as u64,
            )
            .with_attr("name", AttributeValue::String(metrics.name.clone()));
            measurements.push(p_usr);
        }
        if let Some(value_sys) = diff_sys {
            let p_sys: MeasurementPoint = MeasurementPoint::new(
                timestamp,
                self.time_used_system_mode,
                Resource::LocalMachine,
                consumer.clone(),
                value_sys as u64,
            )
            .with_attr("name", AttributeValue::String(metrics.name.clone()));
            measurements.push(p_sys);
        }
        Ok(())
    }
}
