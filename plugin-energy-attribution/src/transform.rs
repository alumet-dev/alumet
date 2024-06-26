use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use alumet::{
    measurement::{MeasurementPoint, WrappedMeasurementValue},
    pipeline::Transform,
    resources::Resource,
};

pub struct EnergyAttributionTransform {
    pub metrics: Arc<Mutex<super::Metrics>>,
    buffer_pod: HashMap<u64, Vec<MeasurementPoint>>,
    buffer_rapl: HashMap<u64, MeasurementPoint>,
}

impl EnergyAttributionTransform {
    pub fn new(metrics: Arc<Mutex<super::Metrics>>) -> Self {
        Self {
            metrics,
            buffer_pod: HashMap::<u64, Vec<MeasurementPoint>>::new(),
            buffer_rapl: HashMap::<u64, MeasurementPoint>::new(),
        }
    }
}

impl Transform for EnergyAttributionTransform {
    fn apply(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
    ) -> Result<(), alumet::pipeline::TransformError> {
        let metrics = self.metrics.lock().unwrap();

        let pod_id = metrics.cpu_usage_per_pod.unwrap();
        let rapl_id = metrics.rapl_consumed_energy.unwrap().as_u64();
        let metric_id = metrics.pod_attributed_energy.unwrap();

        // Filling the buffers
        for m in measurements.clone().iter() {
            if m.metric.as_u64() == rapl_id {
                match m.resource {
                    // If the metric is rapl then we insert only the cpu package one in the buffer
                    Resource::CpuPackage { id: _ } => {
                        let id = m.timestamp.get_sec();

                        self.buffer_rapl.insert(id, m.clone());
                    }
                    _ => continue,
                }
            } else if m.metric.as_u64() == pod_id.as_u64() {
                // Else, if the metric is pod, then we keep only the ones that are prefixed with "pod"
                // before inserting it in the buffer
                if m.attributes().any(|(_, value)| value.to_string().starts_with("pod")) {
                    let id = m.timestamp.get_sec();
                    match self.buffer_pod.get_mut(&id) {
                        Some(vec_points) => {
                            vec_points.push(m.clone());
                        }
                        None => {
                            // If the buffer does not have any value for the current id (timestamp)
                            // then we create the vec with its first value
                            self.buffer_pod.insert(id.clone(), vec![m.clone()]);
                        }
                    }
                }
            }
        }

        // If the buffers do have enough (every) MeasurementPoints
        // Then we compute the energy attribution
        while self.buffer_rapl.len() >= 2 && self.buffer_pod.len() >= 2 {
            let rapl_mini_id = self
                .buffer_rapl
                .keys()
                .reduce(|x, y| if x < y { x } else { y })
                .unwrap()
                .clone();

            if !self.buffer_pod.contains_key(&rapl_mini_id) {
                panic!("pas normal");
            }

            let m = self.buffer_rapl.remove(&rapl_mini_id).unwrap();

            let utime_sum = self
                .buffer_pod
                .get(&rapl_mini_id)
                .unwrap()
                .iter()
                .map(|x| x.clone().value)
                .reduce(|x, y| {
                    match (x, y) {
                        (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => {
                            WrappedMeasurementValue::F64(fx + fy)
                        }
                        (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => {
                            WrappedMeasurementValue::U64(ux + uy)
                        }
                        (_, _) => panic!("Pas normal"), // TODO Fix this panic line
                    }
                })
                .unwrap();

            let utime_sum_f64 = match utime_sum {
                WrappedMeasurementValue::F64(fx) => fx,
                WrappedMeasurementValue::U64(ux) => ux as f64,
            };

            let mut new_m: MeasurementPoint;

            for point in self.buffer_pod.remove(&rapl_mini_id).unwrap().iter() {
                let cur_utime_f64 = match point.value {
                    WrappedMeasurementValue::F64(fx) => fx,
                    WrappedMeasurementValue::U64(ux) => ux as f64,
                };

                new_m = MeasurementPoint::new(
                    m.timestamp,
                    metric_id,
                    point.resource.clone(),
                    point.consumer.clone(),
                    match m.value {
                        WrappedMeasurementValue::F64(fx) => cur_utime_f64 / utime_sum_f64 * fx,
                        WrappedMeasurementValue::U64(ux) => cur_utime_f64 / utime_sum_f64 * (ux as f64),
                    },
                );

                for (key, value) in point.clone().attributes() {
                    new_m = new_m.with_attr(key.to_owned(), value.clone());
                }

                measurements.push(new_m.clone());
            }
        }

        Ok(())
    }
}
