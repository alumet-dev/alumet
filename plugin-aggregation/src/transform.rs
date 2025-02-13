use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, UNIX_EPOCH},
};
use std::{collections::HashMap, sync::{Arc, RwLock}, time::Duration};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, UNIX_EPOCH},
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue}, metrics::RawMetricId, pipeline::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
    }, resources::{Resource, ResourceConsumer}
    },
    resources::{Resource, ResourceConsumer},
};

use crate::aggregations::{self};
use crate::aggregations;
use crate::aggregations::{self};

pub struct AggregationTransform {
    /// Interval used to compute the aggregation.
    interval: Duration,

    /// Buffer used to store every measurement point affected by the aggregation.
    internal_buffer:
        HashMap<(RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>), Vec<MeasurementPoint>>, // TODO: improve the attribute key parts P2.

    internal_buffer: HashMap<(u64, ResourceConsumer, Resource), Vec<MeasurementPoint>>, // TODO: add attributes to the key
    
    internal_buffer:
        HashMap<(RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>), Vec<MeasurementPoint>>, // TODO: improve the attribute key parts P2.

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
    metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,

    /// Aggregation function.
    function: fn(Vec<MeasurementPoint>) -> WrappedMeasurementValue,
}

impl AggregationTransform {
    /// Instantiates a new instance of the aggregation transform plugin.
    /// Instantiates a new instance of the aggregation transform plugin. 
    /// Instantiates a new instance of the aggregation transform plugin.
    pub fn new(
        interval: Duration,
        function: aggregations::Function,
            interval: Duration,
            function: aggregations::Function,
        interval: Duration,
        function: aggregations::Function,
        metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
    ) -> Self {
            metric_correspondence_table: Arc<RwLock<HashMap<u64, u64>>>,
        ) -> Self {
        metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
    ) -> Self {
        Self {
            interval,
            internal_buffer: HashMap::<
                (RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>),
                Vec<MeasurementPoint>,
            >::new(),
            internal_buffer: HashMap::<(u64,  ResourceConsumer, Resource), Vec<MeasurementPoint>>::new(),
            internal_buffer: HashMap::<
                (RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>),
                Vec<MeasurementPoint>,
            >::new(),
            metric_correspondence_table,
            function: function.get_function(),
        }
    }

    /// Empties the buffer and send the aggregated points to the MeasurementBuffer.
    fn buffer_bouncer(&mut self, measurements: &mut alumet::measurement::MeasurementBuffer) {
        let metric_correspondence_table_clone = Arc::clone(&self.metric_correspondence_table.clone());
        let metric_correspondence_table_read = (*metric_correspondence_table_clone).read().unwrap();

        let mut aggregated_points = MeasurementBuffer::new();
        log::debug!("buffer size: {}", self.internal_buffer.len());

        for key in self.internal_buffer.clone().keys() {
            let values = self.internal_buffer.get_mut(&key).unwrap();
            // TODO: Clean the internal_buffer by deleting the empty values/key P2.
            while contains_enough_data(self.interval, &values) {
                let (i, j) = get_ids(self.interval, &values).unwrap();

                let sub_vec: Vec<MeasurementPoint> = values.drain(i..=j).collect();

                // Compute the value of the aggregated point.
                let value = (self.function)(sub_vec.clone());
                    compute_timestamp(sub_vec[0].timestamp, self.interval),
                    *metric_correspondence_table_read.get(&key.clone().0).unwrap(),

                // Init the new point.
                    compute_timestamp(sub_vec[0].timestamp, self.interval),
                    *metric_correspondence_table_read.get(&key.clone().0).unwrap(),
                let new_point = MeasurementPoint::new_untyped(
                    compute_timestamp(sub_vec[0].timestamp, self.interval),
                    value,
                )
                .with_attr_vec(
                    *metric_correspondence_table_read.get(&key.clone().0).unwrap(),
                    key.clone().2,
                    value,
                )
                .with_attr_vec(
                    sub_vec[0]
                        .attributes()
                    key.clone().1,
                    sub_vec[0]
                        .attributes()
                    value,
                        .collect(),
                );
                )
                .with_attr_vec(
                        .collect(),
                );
                    value)
                    .with_attr_vec(
                    value,
                )
                .with_attr_vec(
                    sub_vec[0]
                        .attributes()
                        sub_vec[0].attributes()
                    sub_vec[0]
                        .attributes()
                        .map(|(key, value)| (key.to_owned(), value.clone()))
                        .collect(),
                );
                        .collect()
                    );
                        .collect(),
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
    values[values.len() - 1]
        .timestamp
        .duration_since(compute_timestamp(values[0].timestamp, interval))
        .unwrap()
        .cmp(&interval)
        .is_ge()
}

/// Get the IDs of the first and last measurement point that are
/// inside the interval window.
fn get_ids(interval: Duration, values: &Vec<MeasurementPoint>) -> anyhow::Result<(usize, usize)> {
    let i: usize = 0;
    let min_timestamp = compute_timestamp(values[0].timestamp, interval);

    let j = values
        .iter()
        .position(|point| {
            point
                .timestamp
                .duration_since(min_timestamp)
                .unwrap()
                .cmp(&interval)
                .is_ge()
        })
        .unwrap();

    Ok((i, j - 1))
}

/// Compute the rounded timestamp of reference_timestamp to the closest one
/// bellow it which is a multiple of interval.
fn compute_timestamp(reference_timestamp: Timestamp, interval: Duration) -> Timestamp {
    let reference_unix_timestamp = reference_timestamp.to_unix_timestamp();

    let reference_timestamp_duration = Duration::new(reference_unix_timestamp.0, reference_unix_timestamp.1);
    let reference_timestamp_nanos = reference_timestamp_duration.as_nanos();
    let k = reference_timestamp_nanos / interval.as_nanos();

    let new_ts = UNIX_EPOCH + interval.mul_f64(k as f64);

    Timestamp::from(new_ts)
}
    let first_value = values[0].clone();
    let j = values.iter().position(|point| {
        point.timestamp.duration_since(first_value.timestamp).unwrap().cmp(&interval).is_ge()
    }).unwrap();
/// Compute the rounded timestamp of reference_timestamp to the closest one
/// bellow it which is a multiple of interval.
fn compute_timestamp(reference_timestamp: Timestamp, interval: Duration) -> Timestamp {
    let reference_unix_timestamp = reference_timestamp.to_unix_timestamp();

    let reference_timestamp_duration = Duration::new(reference_unix_timestamp.0, reference_unix_timestamp.1);
    let reference_timestamp_nanos = reference_timestamp_duration.as_nanos();

    // The formula used is:
    // reference_timestamp = k * interval + p, (k,p) ∈ N²
    // => p = reference_timestamp % interval
    // => new_timestamp = k * interval = reference_timestamp - p
    let p = reference_timestamp_nanos % interval.as_nanos();

    let new_timestamp_secs = (reference_timestamp_nanos - p) / 1_000_000_000;
    let new_timestamp_nanosecs = (reference_timestamp_nanos - p) % 1_000_000_000;

    let new_timestamp = UNIX_EPOCH + Duration::new(new_timestamp_secs as u64, new_timestamp_nanosecs as u32);
    Timestamp::from(new_timestamp)
}

    (i, j-1)
}
