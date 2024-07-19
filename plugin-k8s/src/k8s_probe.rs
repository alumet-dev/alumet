use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::{MetricCreationError, TypedMetricId},
    pipeline::elements::error::PollError,
    plugin::{
        util::{CounterDiff, CounterDiffUpdate},
        AlumetPluginStart,
    },
    resources::{Resource, ResourceConsumer},
    units::{PrefixedUnit, Unit},
};
use anyhow::Result;

use crate::cgroup_v2::{self, CgroupV2MetricFile};
use crate::parsing_cgroupv2::CgroupV2Metric;

pub struct K8SProbe {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

#[derive(Clone)]
pub struct Metrics {
    pub time_used_tot: TypedMetricId<u64>,
    pub time_used_user_mode: TypedMetricId<u64>,
    pub time_used_system_mode: TypedMetricId<u64>,
}

impl K8SProbe {
    pub fn new(
        metric: Metrics,
        metric_file: CgroupV2MetricFile,
        counter_tot: CounterDiff,
        counter_sys: CounterDiff,
        counter_usr: CounterDiff,
    ) -> K8SProbe {
        K8SProbe {
            cgroup_v2_metric_file: metric_file,
            time_tot: counter_tot,
            time_usr: counter_usr,
            time_sys: counter_sys,
            time_used_tot: metric.time_used_tot,
            time_used_system_mode: metric.time_used_system_mode,
            time_used_user_mode: metric.time_used_user_mode,
        }
    }
}

impl alumet::pipeline::Source for K8SProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut file_buffer = String::new();
        let metrics: CgroupV2Metric = cgroup_v2::gather_value(&mut self.cgroup_v2_metric_file, &mut file_buffer)?;
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
                value_tot,
            )
            .with_attr("uid", AttributeValue::String(metrics.uid.clone()))
            .with_attr("name", AttributeValue::String(metrics.name.clone()))
            .with_attr("namespace", AttributeValue::String(metrics.namespace.clone()))
            .with_attr("node", AttributeValue::String(metrics.node.clone()));
            measurements.push(p_tot);
        }
        if let Some(value_usr) = diff_usr {
            let p_usr: MeasurementPoint = MeasurementPoint::new(
                timestamp,
                self.time_used_user_mode,
                Resource::LocalMachine,
                consumer.clone(),
                value_usr,
            )
            .with_attr("uid", AttributeValue::String(metrics.uid.clone()))
            .with_attr("name", AttributeValue::String(metrics.name.clone()))
            .with_attr("namespace", AttributeValue::String(metrics.namespace.clone()))
            .with_attr("node", AttributeValue::String(metrics.node.clone()));
            measurements.push(p_usr);
        }
        if let Some(value_sys) = diff_sys {
            let p_sys: MeasurementPoint = MeasurementPoint::new(
                timestamp,
                self.time_used_system_mode,
                Resource::LocalMachine,
                consumer.clone(),
                value_sys,
            )
            .with_attr("uid", AttributeValue::String(metrics.uid.clone()))
            .with_attr("name", AttributeValue::String(metrics.name.clone()))
            .with_attr("namespace", AttributeValue::String(metrics.namespace.clone()))
            .with_attr("node", AttributeValue::String(metrics.node.clone()));

            measurements.push(p_sys);
        }
        Ok(())
    }
}

impl Metrics {
    pub fn new(alumet: &mut AlumetPluginStart) -> Result<Self, MetricCreationError> {
        let usec: PrefixedUnit = PrefixedUnit::micro(Unit::Second);
        Ok(Self {
            time_used_tot: alumet.create_metric::<u64>(
                "total_usage_usec",
                usec.clone(),
                "Total CPU usage time by the group",
            )?,
            time_used_user_mode: alumet.create_metric::<u64>(
                "user_usage_usec",
                usec.clone(),
                "User CPU usage time by the group",
            )?,
            time_used_system_mode: alumet.create_metric::<u64>(
                "system_usage_usec",
                usec.clone(),
                "System CPU usage time by the group",
            )?,
        })
    }
}
