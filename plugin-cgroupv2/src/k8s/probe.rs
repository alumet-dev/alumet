use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::elements::error::PollError,
    plugin::util::CounterDiff,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Result;

use crate::cgroupv2::{CgroupV2Metric, Metrics};

use super::utils::{gather_value, CgroupV2MetricFile};

pub struct K8SProbe {
    pub cgroup_v2_metric_file: CgroupV2MetricFile,
    pub time_tot: CounterDiff,
    pub time_usr: CounterDiff,
    pub time_sys: CounterDiff,
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
    ) -> anyhow::Result<K8SProbe> {
        return Ok(K8SProbe {
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

impl alumet::pipeline::Source for K8SProbe {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        let mut file_buffer = String::new();
        let metrics: CgroupV2Metric = gather_value(&mut self.cgroup_v2_metric_file, &mut file_buffer)?;
        let diff_tot = self.time_tot.update(metrics.time_used_tot).difference();
        let diff_usr = self.time_usr.update(metrics.time_used_user_mode).difference();
        let diff_sys = self.time_sys.update(metrics.time_used_system_mode).difference();
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
