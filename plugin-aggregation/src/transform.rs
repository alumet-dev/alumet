use std::{collections::HashMap, io, time::Duration};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
};

pub struct AggregationTransform {
    interval: Duration,

    internal_buffer: HashMap<(u64, String, String), Vec<MeasurementPoint>>,
}

impl AggregationTransform {
    pub fn new(interval: Duration) -> io::Result<Self> {
        Ok(Self {
            interval,
            internal_buffer: HashMap::<(u64, String, String), Vec<MeasurementPoint>>::new(),
        })
    }
}

impl Transform for AggregationTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // store the measurementBuffer to the internal_buffer
        for measurement in measurements.iter() {
            let id = (
                measurement.metric.as_u64(),
                measurement.consumer.id_string().unwrap_or_default(),
                measurement.resource.id_string().unwrap_or_default(),
            );

            match self.internal_buffer.get_mut(&id) {
                Some(vec_points) => {
                    vec_points.push(measurement.clone());
                }
                None => {
                    self.internal_buffer.insert(id.clone(), vec![measurement.clone()]);
                }
            }
        }

        // clear the measurementBuffer
        measurements.clear();

        for (key, value) in self.internal_buffer.clone().into_iter() {
            if value
                .last()
                .unwrap()
                .timestamp
                .duration_since(value.first().unwrap().timestamp)?
                > self.interval
            {
                let sum = self
                    .internal_buffer
                    .remove(&key)
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

                let mut value_clone = value.first().unwrap().clone();
                value_clone.value = sum.clone();

                // And fill it again
                measurements.push(value_clone.clone());
            }
        }
        Ok(())
    }
}
