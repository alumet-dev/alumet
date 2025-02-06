use std::{collections::HashMap, sync::{Arc, RwLock}, time::Duration};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue}, metrics::RawMetricId, pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    }, resources::{Resource, ResourceConsumer}
};

use crate::aggregations;

pub struct AggregationTransform {
    /// Interval used to compute the aggregation.
    interval: Duration,

    /// Buffer used to store every measurement point affected by the aggregation.
    internal_buffer: HashMap<(u64, ResourceConsumer, Resource), Vec<MeasurementPoint>>, // TODO: add attributes to the key
    
    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,

    /// Aggregation function.
    function: fn(Vec<MeasurementPoint>) -> WrappedMeasurementValue,
}

impl AggregationTransform {
    /// Instantiates a new instance of the aggregation transform plugin. 
    pub fn new(
            interval: Duration,
            function: aggregations::Function,
            metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
        ) -> Self {
        Self {
            interval,
            internal_buffer: HashMap::<(u64,  ResourceConsumer, Resource), Vec<MeasurementPoint>>::new(),
            metric_correspondence_table,
            function: function.get_function(),
        }
    }

    /// Empties the buffer and send the aggregated points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        let mut aggregated_points = MeasurementBuffer::new();

        for (key, mut values) in self.internal_buffer.clone() {
            while contains_enough_data(self.interval, &values) {
                let (i,j) = get_ids(self.interval, &values);

                let sub_vec: Vec<MeasurementPoint> = values.drain(i..j).collect();

                // Compute the value of the aggregated point.
                let value = (self.function)(sub_vec.clone());

                // Init the new point.
                let new_point = MeasurementPoint::new_untyped(
                    Timestamp::now(), // TODO: compute this timestamp based on the interval P1
                    RawMetricId::from_u64(key.clone().0),
                    key.clone().2,
                    key.clone().1,
                    value)
                    .with_attr_vec(
                        sub_vec[0].attributes()
                        .map(|(key, value)| (key.to_owned(), value.clone()))
                        .collect()
                    );

                // Push the new point to the result buffer.
                aggregated_points.push(new_point);
            }
        }

        measurements.merge(&mut aggregated_points);
    }
}

impl Transform for AggregationTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _: &TransformContext) -> Result<(), TransformError> {
        let metric_correspondence_table_clone = Arc::clone(&self.metric_correspondence_table.clone());
        let metric_correspondence_table_read = (*metric_correspondence_table_clone).read().unwrap();

        let mut not_needed_measurement_point = MeasurementBuffer::new();

        // Store the measurementBuffer needed metrics to the internal_buffer.
        for measurement in measurements.iter() {
            let id = (
                measurement.metric.as_u64(),
                measurement.consumer.clone(),
                measurement.resource.clone(),
            );

            // If metric id not needed, then skip it.
            if !(*metric_correspondence_table_read).contains_key(&measurement.metric) {
                not_needed_measurement_point.push(measurement.clone());
                continue;
            }

            let id = (
                measurement.metric,
                measurement.consumer.clone(),
                measurement.resource.clone(),
                measurement
                    .attributes()
                    .map(|attribute| (attribute.0.to_string(), attribute.1.clone()))
                    .collect::<Vec<(String, AttributeValue)>>(),
            );

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
    }
}

/// Returns true if the vec contains enough data to compute the aggregation
/// for the configured window.
fn contains_enough_data(interval: Duration, values: &Vec<MeasurementPoint>) -> bool {
    values[values.len()-1]
        .timestamp
        .duration_since(values[0].timestamp)
        .unwrap()
        .cmp(&interval)
        .is_ge()
}

/// Get the IDs of the first and last measurement point that are
/// inside the interval window.
fn get_ids(interval: Duration, values: &Vec<MeasurementPoint>) -> (usize, usize) {
    let i: usize = 0;
    let first_value = values[0].clone();
    let j = values.iter().position(|point| {
        point.timestamp.duration_since(first_value.timestamp).unwrap().cmp(&interval).is_ge()
    }).unwrap();

    (i, j-1)
}
