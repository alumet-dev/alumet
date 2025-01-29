use std::{collections::HashMap, sync::{Arc, RwLock}, time::Duration};

use alumet::{
    measurement::{self, MeasurementBuffer, MeasurementPoint, Timestamp}, metrics::TypedMetricId, pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    }, resources::{Resource, ResourceConsumer}
};

pub struct AggregationTransform {
    interval: Duration,

    internal_buffer: HashMap<(u64, ResourceConsumer, Resource), Vec<MeasurementPoint>>,
    metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
    typed_metric_ids: Arc<RwLock<HashMap<u64, TypedMetricId<u64>>>>,
}

impl AggregationTransform {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            internal_buffer: HashMap::<(u64,  ResourceConsumer, Resource), Vec<MeasurementPoint>>::new(),
            metric_correspondence_table: Arc::new(RwLock::new(HashMap::<u64, u64>::new())), // TODO: init this arc new in the lib.rs
            typed_metric_ids: Arc::new(RwLock::new(HashMap::<u64, TypedMetricId<u64>>::new()))
        }
    }

    /// Empties the buffer and send the aggregated points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        let mut aggregated_points = MeasurementBuffer::new();

        let typed_metric_ids_clone = Arc::clone(&self.typed_metric_ids.clone());
        let bis = (*typed_metric_ids_clone).read().unwrap();


        for (key, values) in self.internal_buffer.clone() {
            // TODO: check if values is big enough
            // then apply the calculation to the sub vec
            // and add the aggregated point to the aggregated_points buffer.
            // let typed_metric = values[0].;
            let new_point = MeasurementPoint::new(
                Timestamp::now(),
                *(*bis).get(&key.0).unwrap(),
                key.2,
                key.1,
                0);
            
            aggregated_points.push(new_point);
        }

        measurements.merge(&mut aggregated_points);
    }
}

impl Transform for AggregationTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _: &TransformContext) -> Result<(), TransformError> {
        let metric_correspondence_table_clone = Arc::clone(&self.metric_correspondence_table.clone());
        let bis = (*metric_correspondence_table_clone).read().unwrap();

        let mut not_needed_measurement_point = MeasurementBuffer::new();

        // Store the measurementBuffer needed metrics to the internal_buffer.
        for measurement in measurements.iter() {
            let id = (
                measurement.metric.as_u64(),
                measurement.consumer.clone(),
                measurement.resource.clone(),
            );

            // If metric id not needed, then skip it.
            if !(*bis).contains_key(&id.0) {
                not_needed_measurement_point.push(measurement.clone());
                continue;
            }

            // Add the measurement point to the internal buffer.
            match self.internal_buffer.get_mut(&id) {
                Some(vec_points) => {
                    vec_points.push(measurement.clone());
                }
                None => {
                    self.internal_buffer.insert(id.clone(), vec![measurement.clone()]);
                }
            }
        }

        // clear the measurementBuffer if needed (see TODO on config boolean)
        measurements.clear();

        // Fill it again with the not needed points.
        measurements.merge(&mut not_needed_measurement_point);
        
        self.buffer_bouncer(measurements);

        Ok(())

        // TODO: implement the sum function
        // TODO: implement the mean function

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
    }
}
