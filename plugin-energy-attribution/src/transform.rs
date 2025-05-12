use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
    timeseries::grouped_buffer::GroupedBuffer,
};

pub struct EnergyAttributionTransform {
    metrics: super::AttributionMetrics,
    grouped_buffer: GroupedBuffer<(RawMetricId, Resource, ResourceConsumer)>, // TODO récupérer les métriques
}

impl Transform for EnergyAttributionTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        self.grouped_buffer.extend(measurements);
        self.grouped_buffer.extract_min_all();
        let energy = self.grouped_buffer.take(energy_metric);
        let consumption = self.grouped_buffer.take(consumption_metric);
        let consumption = consumption.interpolate(energy);
        // let result = energy*consumption

        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        let (pod_id, rapl_id) = {
            let metrics = &self.metrics;

            let pod_id = metrics.hardware_usage.as_u64();
            let rapl_id = metrics.consumed_energy.as_u64();

            (pod_id, rapl_id)
        };

        // Filling the buffers.
        for m in measurements.clone().iter() {
            if m.metric.as_u64() == rapl_id {
                match m.resource {
                    // If the metric is rapl then we insert only the cpu package one in the buffer.
                    Resource::CpuPackage { id: _ } => {
                        let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_secs();

                        self.buffer_rapl.insert(id, m.clone());
                    }
                    _ => continue,
                }
            } else if m.metric.as_u64() == pod_id {
                // Else, if the metric is pod, then we keep only the ones that are prefixed with "pod"
                // before inserting them in the buffer.
                if m.attributes().any(|(_, value)| value.to_string().starts_with("pod")) {
                    let id = SystemTime::from(m.timestamp).duration_since(UNIX_EPOCH)?.as_secs();
                    match self.buffer_pod.get_mut(&id) {
                        Some(vec_points) => {
                            vec_points.push(m.clone());
                        }
                        None => {
                            // If the buffer does not have any value for the current id (timestamp)
                            // then we create the vec with its first value.
                            self.buffer_pod.insert(id, vec![m.clone()]);
                        }
                    }
                }
            }
        }

        // Emptying the buffers and pushing the energy attribution to the MeasurementBuffer
        self.buffer_bouncer(measurements);

        Ok(())
    }
}
