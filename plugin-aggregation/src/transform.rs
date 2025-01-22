use std::{any::Any, collections::HashMap, io, time::Duration};

use std::sync::Arc;

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint}, metrics::{self, MetricId}, pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    }
};

pub struct AggregationTransform {
    interval: Duration,

    internal_buffer: HashMap<(u64, String, String), Vec<MeasurementPoint>>,
    metric_correspondance_table: Arc<HashMap<u64, u64>>,
}

impl AggregationTransform {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            internal_buffer: HashMap::<(u64, String, String), Vec<MeasurementPoint>>::new(),
            metric_correspondance_table: Arc::new(HashMap::<u64, u64>::new()),
        }
    }
}

impl Transform for AggregationTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, ctx: &TransformContext) -> Result<(), TransformError> {
        // store the measurementBuffer to the internal_buffer
        for measurement in measurements.iter() {
            if self.metric_correspondance_table.get(&measurement.metric.as_u64()).is_none() {
                let current_metric = ctx.metrics.by_id(&measurement.metric.untyped_id()).unwrap();

            }

            let id = (
                measurement.metric.as_u64(),
                measurement.consumer.id_string().unwrap_or_default(),
                measurement.resource.id_string().unwrap_or_default(),
            );

            match self.internal_buffer.get_mut(&id) {
                Some(vec_points) => {
                    let current_interval: (u64, u64) = get_current_interval(self.interval.as_secs(), measurement.timestamp.to_unix_timestamp().0); 
                    // vec_points.push(measurement.clone());
                }
                None => {
                    // self.internal_buffer.insert(id.clone(), vec![measurement.clone()]);
                    self.internal_buffer.insert(id.clone(), vec![measurement.clone()]);
                }
            }
        }

        // clear the measurementBuffer
        measurements.clear();

        // for (key, value) in self.internal_buffer.clone().into_iter() {
        //     if value
        //         .last()
        //         .unwrap()
        //         .timestamp
        //         .duration_since(value.first().unwrap().timestamp)?
        //         > self.interval
        //     {
        //         let sum = self
        //             .internal_buffer
        //             .remove(&key)
        //             .unwrap()
        //             .iter()
        //             .map(|x| x.clone().value)
        //             .reduce(|x, y| {
        //                 match (x, y) {
        //                     (WrappedMeasurementValue::F64(fx), WrappedMeasurementValue::F64(fy)) => {
        //                         WrappedMeasurementValue::F64(fx + fy)
        //                     }
        //                     (WrappedMeasurementValue::U64(ux), WrappedMeasurementValue::U64(uy)) => {
        //                         WrappedMeasurementValue::U64(ux + uy)
        //                     }
        //                     (_, _) => panic!("Pas normal"), // TODO Fix this panic line
        //                 }
        //             })
        //             .unwrap();

        //         let mut value_clone = value.first().unwrap().clone();
        //         value_clone.value = sum.clone();

        //         // And fill it again
        //         measurements.push(value_clone.clone());
        //     }
        // }
        Ok(())
    }
}

fn get_current_interval(interval: u64, timestamp: u64) -> (u64, u64) {
    // (t, t+int) where t = k * int, k € N
    let t = timestamp / interval;
    (t, t + interval)
}