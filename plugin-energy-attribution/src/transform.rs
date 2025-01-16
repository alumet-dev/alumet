use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::Resource,
};

pub struct EnergyAttributionTransform {
    pub metrics: super::Metrics,
    buffer_pod: HashMap<u64, Vec<MeasurementPoint>>,
    buffer_rapl: HashMap<u64, MeasurementPoint>,
}
impl EnergyAttributionTransform {
    /// Instantiates a new EnergyAttributionTransform with its private fields initialized.
    pub fn new(metrics: super::Metrics) -> Self {
        Self {
            metrics,
            buffer_pod: HashMap::<u64, Vec<MeasurementPoint>>::new(),
            buffer_rapl: HashMap::<u64, MeasurementPoint>::new(),
        }
    }

    /// Empties the buffers and send the energy attribution points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        // Retrieving the metric_id of the energy attribution.
        // Using a nested scope to reduce the lock time.
        let metric_id = self.metrics.pod_attributed_energy;

        // If the buffers do have enough (every) MeasurementPoints,
        // then we compute the energy attribution.
        while self.buffer_rapl.len() >= 2 && self.buffer_pod.len() >= 2 {
            // Get the smallest rapl id i.e. the oldest timestamp (key) present in the buffer.
            let rapl_mini_id = self
                .buffer_rapl
                .keys()
                .reduce(|x, y| if x < y { x } else { y })
                .unwrap()
                .clone();

            // Check if the buffer_pod contains the key to prevent any panic/error bellow.
            if !self.buffer_pod.contains_key(&rapl_mini_id) {
                todo!("decide what to do in this case");
            }

            let rapl_point = self.buffer_rapl.remove(&rapl_mini_id).unwrap();

            // Compute the sum of every `total_usage_usec` for the given timestamp: `rapl_mini_id`.
            let tot_time_sum = self
                .buffer_pod
                .get(&rapl_mini_id)
                .unwrap()
                .iter()
                .map(|x| match x.value {
                    WrappedMeasurementValue::F64(fx) => fx,
                    WrappedMeasurementValue::U64(ux) => ux as f64,
                })
                .sum::<f64>();

            // Then for every points in the buffer_pod at `rapl_mini_id`.
            for point in self.buffer_pod.remove(&rapl_mini_id).unwrap().iter() {
                // We extract the current tot_time as f64.
                let cur_tot_time_f64 = match point.value {
                    WrappedMeasurementValue::F64(fx) => fx,
                    WrappedMeasurementValue::U64(ux) => ux as f64,
                };

                // Extract the attributes of the current point to add them
                // to the new measurement point.
                let point_attributes = point
                    .attributes()
                    .map(|(key, value)| (key.to_owned(), value.clone()))
                    .collect();

                // We create the new MeasurementPoint for the energy attribution.
                let new_m = MeasurementPoint::new(
                    rapl_point.timestamp,
                    metric_id,
                    point.resource.clone(),
                    point.consumer.clone(),
                    match rapl_point.value {
                        WrappedMeasurementValue::F64(fx) => cur_tot_time_f64 / tot_time_sum * fx,
                        WrappedMeasurementValue::U64(ux) => cur_tot_time_f64 / tot_time_sum * (ux as f64),
                    },
                )
                .with_attr_vec(point_attributes);

                // And finally, the MeasurementPoint is pushed to the MeasurementBuffer.
                measurements.push(new_m.clone());
            }
        }
    }
}

impl Transform for EnergyAttributionTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
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
