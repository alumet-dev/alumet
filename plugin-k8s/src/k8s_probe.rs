use alumet::{
    measurement::{AttributeValue, Timestamp, MeasurementAccumulator, MeasurementPoint},
    metrics::{TypedMetricId,MetricCreationError},
    plugin::{util::{CounterDiff,CounterDiffUpdate}, AlumetStart},
    units::Unit,
    resources::ResourceId,
};
use anyhow::Result;

use crate::cgroup_v2::{self, CgroupV2MetricFile};
use crate::parsing_cgroupv2::CgroupV2Metric;

pub(crate) const CGROUP_MAX_TS: u64 = u64::MAX;


/// Energy probe based on perf_event for intel RAPL.
pub struct K8SProbe {
    pub metrics: Metrics,
    pub metric_and_counter: Vec<(CgroupV2MetricFile, CounterDiff, CounterDiff, CounterDiff)>,
}

#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

impl K8SProbe {
    pub fn new(metric: Metrics, final_li_metric: Vec<CgroupV2MetricFile>) -> anyhow::Result<K8SProbe> {
        let mut metric_counter: Vec<(CgroupV2MetricFile, CounterDiff, CounterDiff, CounterDiff)> = Vec::new();
        for elm in final_li_metric { //elm is  a CgroupV2MetricFile
            let counter_tmp_tot = CounterDiff::with_max_value(CGROUP_MAX_TS);
            let counter_tmp_usr = CounterDiff::with_max_value(CGROUP_MAX_TS);
            let counter_tmp_sys = CounterDiff::with_max_value(CGROUP_MAX_TS);
            metric_counter.push((elm, counter_tmp_tot, counter_tmp_usr, counter_tmp_sys));            
        }
        return Ok(K8SProbe{
        metrics: metric,
        metric_and_counter: metric_counter,
        });
    }
}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(
        &mut self,
        measurements: &mut MeasurementAccumulator,
        timestamp: Timestamp,
    ) -> Result<(), alumet::pipeline::PollError> {    
        for (metricFile, counter_tot, counter_usr, counter_sys) in &mut self.metric_and_counter {
            let metrics: CgroupV2Metric = cgroup_v2::gather_value(metricFile)?;
            let diff_tot = match counter_tot.update(metrics.time_used_tot) {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            let diff_usr = match counter_usr.update(metrics.time_used_user_mode) {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            let diff_sys = match counter_sys.update(metrics.time_used_system_mode) {
                CounterDiffUpdate::FirstTime => None,
                CounterDiffUpdate::Difference(diff) => Some(diff),
                CounterDiffUpdate::CorrectedDifference(diff) => Some(diff),
            };
            if let Some(value_tot) = diff_tot {
                let p_tot: MeasurementPoint = MeasurementPoint::new(timestamp, self.metrics.time_used_tot, ResourceId::LocalMachine, value_tot as u64).with_attr("pod", AttributeValue::String(metrics.name.to_string()));
                measurements.push(p_tot);
            }
            if let Some(value_usr) = diff_usr{
                let p_usr: MeasurementPoint = MeasurementPoint::new(timestamp, self.metrics.time_used_user_mode, ResourceId::LocalMachine, value_usr as u64).with_attr("pod", AttributeValue::String(metrics.name.to_string()));
                measurements.push(p_usr);
            }
            if let Some(value_sys) = diff_sys{
                let p_sys: MeasurementPoint = MeasurementPoint::new(timestamp, self.metrics.time_used_system_mode, ResourceId::LocalMachine, value_sys as u64).with_attr("pod", AttributeValue::String(metrics.name.to_string()));
                measurements.push(p_sys);
            }
        }
        Ok(())
    }
}


impl Metrics {
    pub fn new(alumet: &mut AlumetStart) -> Result<Self, MetricCreationError> {
        let usec = Unit::Custom {
            unique_name: "usec".to_owned(),
            display_name: "µsec".to_owned(),
        };
        Ok(Self {
        time_used_tot: alumet.create_metric::<u64>("total_usage_usec", usec.clone(), "Total CPU usage time by the group")?, 
        time_used_user_mode: alumet.create_metric::<u64>("user_usage_usec", usec.clone(), "User CPU usage time by the group")?,
        time_used_system_mode: alumet.create_metric::<u64>("system_usage_usec", usec.clone(), "System CPU usage time by the group")?,
        })
    }
}