use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, UNIX_EPOCH},
};

use anyhow::anyhow;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
};

use crate::aggregations::{self};

pub struct AggregationTransform {
    /// Interval used to compute the aggregation.
    interval: Duration,

    /// Buffer used to store every measurement point affected by the aggregation.
    internal_buffer:
        HashMap<(RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>), Vec<MeasurementPoint>>, // TODO: improve the attribute key parts P2.

    /// Store the correspondence table between aggregated metrics and the original ones.
    /// The key is the original metric's id and the value is the id of the aggregated metric.
    metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,

    /// Aggregation function.
    function: fn(Vec<MeasurementPoint>) -> Option<WrappedMeasurementValue>,
}

impl AggregationTransform {
    /// Instantiates a new instance of the aggregation transform plugin.
    pub fn new(
        interval: Duration,
        function: aggregations::Function,
        metric_correspondence_table: Arc<RwLock<HashMap<RawMetricId, RawMetricId>>>,
    ) -> Self {
        Self {
            interval,
            internal_buffer: HashMap::new(),
            metric_correspondence_table,
            function: function.function(),
        }
    }

    /// Empties the buffer and send the aggregated points to the MeasurementBuffer.
    fn buffer_bouncer(
        &mut self,
        measurements: &mut alumet::measurement::MeasurementBuffer,
    ) -> Result<(), TransformError> {
        let metric_correspondence_table_clone = &self.metric_correspondence_table.clone();
        let metric_correspondence_table_read = metric_correspondence_table_clone
            .read()
            .map_err(|_| anyhow!("could not read the metric correspondence table"))?;

        let mut aggregated_points = MeasurementBuffer::new();
        log::debug!("buffer size: {}", self.internal_buffer.len());

        for (key, values) in &mut self.internal_buffer {
            // TODO: Clean the internal_buffer by deleting the empty values/key P2.
            loop {
                let min_timestamp = compute_min_timestamp(values[0].timestamp, self.interval);

                if !contains_enough_data(self.interval, &values, min_timestamp) {
                    break;
                }

                let (i, j) = get_ids(self.interval, &values, min_timestamp)?;

                let sub_vec: Vec<MeasurementPoint> = values.drain(i..=j).collect();

                // Compute the value of the aggregated point.
                let Some(value) = (self.function)(sub_vec.clone()) else {
                    return Err(TransformError::UnexpectedInput(anyhow!(
                        "could not compute the aggregation for the sub_vec of {key:?}"
                    )));
                };

                // Init the new point.
                let new_point = MeasurementPoint::new_untyped(
                    compute_min_timestamp(sub_vec[0].timestamp, self.interval),
                    *metric_correspondence_table_read
                        .get(&key.clone().0)
                        .ok_or(TransformError::UnexpectedInput(anyhow!(
                            "the metric ID {} is not known by the correspondence table",
                            &key.clone().0.as_u64()
                        )))?,
                    key.clone().2,
                    key.clone().1,
                    value,
                )
                .with_attr_vec(
                    sub_vec[0]
                        .attributes()
                        .map(|(key, value)| (key.to_owned(), value.clone()))
                        .collect(),
                );

                // Push the new point to the result buffer.
                aggregated_points.push(new_point);
            }
        }

        measurements.merge(&mut aggregated_points);

        Ok(())
    }
}

impl Transform for AggregationTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _: &TransformContext) -> Result<(), TransformError> {
        let metric_correspondence_table_clone = &self.metric_correspondence_table.clone();
        let metric_correspondence_table_read = metric_correspondence_table_clone
            .read()
            .expect("metric_correspondence_table lock poisoned");

        let mut not_needed_measurement_point = MeasurementBuffer::new();

        // Store the measurementBuffer needed metrics to the internal_buffer.
        for measurement in measurements.iter() {
            // If metric id not needed, then skip it.
            if !metric_correspondence_table_read.contains_key(&measurement.metric) {
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

        self.buffer_bouncer(measurements)
    }
}

/// Returns true if the vec contains enough data to compute the aggregation
/// for the configured window.
fn contains_enough_data(interval: Duration, values: &Vec<MeasurementPoint>, min_timestamp: Timestamp) -> bool {
    let elapsed = values[values.len() - 1]
        .timestamp
        .duration_since(min_timestamp)
        .unwrap();
    elapsed >= interval
}

/// Get the IDs of the first and last measurement point that are
/// inside the interval window.
fn get_ids(
    interval: Duration,
    values: &Vec<MeasurementPoint>,
    min_timestamp: Timestamp,
) -> anyhow::Result<(usize, usize)> {
    let i: usize = 0;

    let Some(j) = values.iter().position(|point| {
        let elapsed = point.timestamp.duration_since(min_timestamp).unwrap();
        elapsed >= interval
    }) else {
        return Err(anyhow!("could not compute the IDs for the current sub_vec"));
    };

    Ok((i, j - 1))
}

/// Compute the rounded timestamp of reference_timestamp to the closest one
/// bellow it which is a multiple of interval.
fn compute_min_timestamp(reference_timestamp: Timestamp, interval: Duration) -> Timestamp {
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

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use time::{format_description::well_known::Rfc3339, OffsetDateTime};

    use alumet::{
        measurement::{MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
    };

    use crate::transform::{compute_min_timestamp, contains_enough_data};

    use super::get_ids;

    /// Parses an RFC 3339 date-and-time string into a Timestamp value.
    pub(crate) fn timestamp_from_rfc3339(timestamp: &str) -> Timestamp {
        SystemTime::from(OffsetDateTime::parse(timestamp, &Rfc3339).unwrap()).into()
    }

    pub(crate) fn new_point(timestamp: &str, value: WrappedMeasurementValue, id: u64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            timestamp_from_rfc3339(timestamp),
            RawMetricId::from_u64(id),
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            value,
        )
    }

    fn measurement_buffer_to_comparable_vec(
        measurement_buffer: MeasurementBuffer,
    ) -> Vec<(Timestamp, WrappedMeasurementValue, u64)> {
        let mut new_list = measurement_buffer
            .iter()
            .map(|point| (point.timestamp, point.value.clone(), point.metric.as_u64()))
            .collect::<Vec<(Timestamp, WrappedMeasurementValue, u64)>>();

        new_list.sort_by_key(|tuple| (SystemTime::from(tuple.0), tuple.2));
        new_list
    }

    #[test]
    fn test_compute_min_timestamp() {
        let reference_date = timestamp_from_rfc3339("2025-02-10 13:19:05.87Z");
        let mut expected_date = timestamp_from_rfc3339("2025-02-10T13:19:00Z");

        // Compute the round timestamp with a 1 minute interval.
        assert_eq!(
            compute_min_timestamp(reference_date, Duration::from_secs(60)),
            expected_date,
            "{reference_date:?} should be rounded to {expected_date:?} with the given interval of 1 minute",
        );

        expected_date = timestamp_from_rfc3339("2025-02-10T13:19:05Z");

        // Compute the round timestamp with a 1 second interval.
        assert_eq!(
            compute_min_timestamp(reference_date, Duration::from_secs(1)),
            expected_date,
            "{reference_date:?} should be rounded to {expected_date:?} with the given interval of 1 second",
        );

        expected_date = timestamp_from_rfc3339("2025-02-10T13:19:05.80Z");

        // Compute the round timestamp with a 100 milliseconds interval.
        assert_eq!(
            compute_min_timestamp(reference_date, Duration::from_millis(100)),
            expected_date
        );
    }

    #[test]
    fn test_get_ids() {
        let test_list = vec![
            new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:05Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:10Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:17Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:20Z", WrappedMeasurementValue::U64(0), 0),
        ];

        let min_timestamp = timestamp_from_rfc3339("2025-02-10T13:19:00Z");

        assert_eq!(
            get_ids(Duration::from_secs(1), &test_list, min_timestamp).unwrap(),
            (0, 0)
        );
        assert_eq!(
            get_ids(Duration::from_secs(6), &test_list, min_timestamp).unwrap(),
            (0, 2)
        );
        assert_eq!(
            get_ids(Duration::from_secs(10), &test_list, min_timestamp).unwrap(),
            (0, 2)
        );
        assert_eq!(
            get_ids(Duration::from_secs(15), &test_list, min_timestamp).unwrap(),
            (0, 3)
        );
        assert_eq!(
            get_ids(Duration::from_secs(20), &test_list, min_timestamp).unwrap(),
            (0, 4)
        );

        assert_eq!(
            get_ids(
                Duration::from_secs(1),
                &test_list[0..=1].to_vec(),
                timestamp_from_rfc3339("2025-02-10T13:19:00Z")
            )
            .unwrap(),
            (0, 0)
        );

        assert!(get_ids(
            Duration::from_secs(1),
            &Vec::<MeasurementPoint>::new(),
            timestamp_from_rfc3339("2025-02-10T13:19:00Z")
        )
        .is_err());
    }

    #[test]
    fn test_contains_enough_data() {
        let test_list = vec![
            new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:05Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:10Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:17Z", WrappedMeasurementValue::U64(0), 0),
            new_point("2025-02-10T13:19:20Z", WrappedMeasurementValue::U64(0), 0),
        ];

        let min_timestamp = timestamp_from_rfc3339("2025-02-10T13:19:00Z");

        assert!(contains_enough_data(Duration::from_secs(1), &test_list, min_timestamp));
        assert!(contains_enough_data(Duration::from_secs(5), &test_list, min_timestamp));
        assert!(contains_enough_data(Duration::from_secs(10), &test_list, min_timestamp));
        assert!(contains_enough_data(Duration::from_secs(15), &test_list, min_timestamp));
        assert!(contains_enough_data(Duration::from_secs(20), &test_list, min_timestamp));
        assert!(!contains_enough_data(
            Duration::from_secs(60),
            &test_list,
            min_timestamp
        ));
    }

    mod buffer_bouncer {
        use std::{
            collections::HashMap,
            sync::{Arc, RwLock},
            thread,
            time::Duration,
        };

        use anyhow::anyhow;

        use alumet::{
            measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
            metrics::RawMetricId,
            pipeline::elements::error::TransformError,
            resources::{Resource, ResourceConsumer},
        };

        use crate::{
            aggregations,
            transform::{
                tests::{measurement_buffer_to_comparable_vec, new_point, timestamp_from_rfc3339},
                AggregationTransform,
            },
        };

        #[test]
        fn empty_buffer() {
            let mut transform_plugin = AggregationTransform::new(
                Duration::from_secs(10),
                aggregations::Function::Mean,
                Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::new())),
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            transform_plugin.buffer_bouncer(&mut measurement_buffer).unwrap();

            assert_eq!(measurement_buffer.len(), 0)
        }

        #[test]
        fn buffer_with_data() {
            let mut transform_plugin = AggregationTransform::new(
                Duration::from_secs(10),
                aggregations::Function::Mean,
                Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::from([
                    (RawMetricId::from_u64(1), RawMetricId::from_u64(4)),
                    (RawMetricId::from_u64(2), RawMetricId::from_u64(7)),
                ]))),
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            // Add first list of measurement points.
            let key_1 = (
                RawMetricId::from_u64(1),
                ResourceConsumer::LocalMachine,
                Resource::LocalMachine,
                Vec::<(String, AttributeValue)>::new(),
            );
            transform_plugin.internal_buffer.insert(
                key_1.clone(),
                vec![
                    new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 1),
                    new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(1), 1),
                    new_point("2025-02-10T13:19:05Z", WrappedMeasurementValue::U64(18), 1),
                    new_point("2025-02-10T13:19:10Z", WrappedMeasurementValue::U64(3), 1),
                    new_point("2025-02-10T13:19:17Z", WrappedMeasurementValue::U64(6), 1),
                    new_point("2025-02-10T13:19:20Z", WrappedMeasurementValue::U64(0), 1),
                ],
            );

            transform_plugin.internal_buffer.get_mut(&key_1).unwrap()[0].add_attr("test", "unit");

            let key_2 = (
                RawMetricId::from_u64(2),
                ResourceConsumer::LocalMachine,
                Resource::LocalMachine,
                Vec::<(String, AttributeValue)>::new(),
            );

            // Add second list of measurement points.
            transform_plugin.internal_buffer.insert(
                key_2.clone(),
                vec![
                    new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 2),
                    new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(1), 2),
                    new_point("2025-02-10T13:19:05Z", WrappedMeasurementValue::U64(18), 2),
                    new_point("2025-02-10T13:19:06Z", WrappedMeasurementValue::U64(3), 2),
                    new_point("2025-02-10T13:19:07Z", WrappedMeasurementValue::U64(6), 2),
                    new_point("2025-02-10T13:19:11Z", WrappedMeasurementValue::U64(0), 2),
                ],
            );

            transform_plugin.buffer_bouncer(&mut measurement_buffer).unwrap();

            assert_eq!(measurement_buffer.len(), 3);

            assert_eq!(transform_plugin.internal_buffer.len(), 2);
            assert!(transform_plugin.internal_buffer.contains_key(&key_1));
            assert!(transform_plugin.internal_buffer.contains_key(&key_2));

            assert_eq!(transform_plugin.internal_buffer.get(&key_2).unwrap().len(), 1);
            assert_eq!(transform_plugin.internal_buffer.get(&key_1).unwrap().len(), 1);

            assert_eq!(
                measurement_buffer_to_comparable_vec(measurement_buffer),
                vec![
                    (
                        timestamp_from_rfc3339("2025-02-10T13:19:00Z"),
                        WrappedMeasurementValue::U64(6),
                        4
                    ),
                    (
                        timestamp_from_rfc3339("2025-02-10T13:19:00Z"),
                        WrappedMeasurementValue::U64(5),
                        7
                    ),
                    (
                        timestamp_from_rfc3339("2025-02-10T13:19:10Z"),
                        WrappedMeasurementValue::U64(4),
                        4
                    ),
                ]
            );
        }

        #[test]
        fn error_in_aggregation_calculation() {
            fn aggregation_returns_none(_: Vec<MeasurementPoint>) -> Option<WrappedMeasurementValue> {
                None
            }

            let mut transform_plugin = AggregationTransform {
                interval: Duration::from_secs(10),
                internal_buffer: HashMap::<
                    (RawMetricId, ResourceConsumer, Resource, Vec<(String, AttributeValue)>),
                    Vec<MeasurementPoint>,
                >::new(),
                metric_correspondence_table: Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::from([
                    (RawMetricId::from_u64(1), RawMetricId::from_u64(4)),
                    (RawMetricId::from_u64(2), RawMetricId::from_u64(7)),
                ]))),
                function: aggregation_returns_none,
            };

            // Add first list of measurement points.
            let key = (
                RawMetricId::from_u64(1),
                ResourceConsumer::LocalMachine,
                Resource::LocalMachine,
                Vec::<(String, AttributeValue)>::new(),
            );
            transform_plugin.internal_buffer.insert(
                key.clone(),
                vec![
                    new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 1),
                    new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(1), 1),
                    new_point("2025-02-10T13:19:05Z", WrappedMeasurementValue::U64(18), 1),
                    new_point("2025-02-10T13:19:10Z", WrappedMeasurementValue::U64(3), 1),
                    new_point("2025-02-10T13:19:17Z", WrappedMeasurementValue::U64(6), 1),
                    new_point("2025-02-10T13:19:20Z", WrappedMeasurementValue::U64(0), 1),
                ],
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            let result = transform_plugin.buffer_bouncer(&mut measurement_buffer);
            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                TransformError::UnexpectedInput(anyhow!(
                    "could not compute the aggregation for the sub_vec of {key:?}"
                ))
                .to_string()
            );
        }

        #[test]
        fn metric_correspondence_table_lock_poisoned() {
            let mut transform_plugin = AggregationTransform::new(
                Duration::from_secs(10),
                aggregations::Function::Mean,
                Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::from([
                    (RawMetricId::from_u64(1), RawMetricId::from_u64(4)),
                    (RawMetricId::from_u64(2), RawMetricId::from_u64(7)),
                ]))),
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            let metric_correspondence_table_clone = Arc::clone(&transform_plugin.metric_correspondence_table.clone());

            let _ = thread::spawn(move || {
                let _lock = (*metric_correspondence_table_clone).write().unwrap();
                panic!(); // The metric_correspondence_table gets poisoned by this panic.
            })
            .join();

            let metric_correspondence_table_clone = Arc::clone(&transform_plugin.metric_correspondence_table.clone());
            assert!((*metric_correspondence_table_clone).is_poisoned()); // Check that the lock is poisoned.

            let result = transform_plugin.buffer_bouncer(&mut measurement_buffer);
            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                TransformError::Fatal(anyhow!("could not read the metric correspondence table")).to_string()
            );
        }
    }

    mod apply {
        use std::{
            collections::HashMap,
            sync::{Arc, RwLock},
            thread,
            time::Duration,
        };

        use alumet::{
            measurement::{AttributeValue, MeasurementBuffer, WrappedMeasurementValue},
            metrics::RawMetricId,
            pipeline::{elements::transform::TransformContext, Builder, Transform},
            resources::{Resource, ResourceConsumer},
        };

        use crate::{
            aggregations,
            transform::{
                tests::{measurement_buffer_to_comparable_vec, timestamp_from_rfc3339},
                AggregationTransform,
            },
        };

        use super::new_point;

        #[test]
        fn test_apply() {
            let builder: Builder = Builder::new();
            let inspector = builder.inspect();
            let test_tranform_context: TransformContext = TransformContext {
                metrics: &inspector.metrics(),
            };

            let mut transform_plugin = AggregationTransform::new(
                Duration::from_secs(10),
                aggregations::Function::Sum,
                Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::from([
                    (RawMetricId::from_u64(0), RawMetricId::from_u64(4)),
                    (RawMetricId::from_u64(2), RawMetricId::from_u64(7)),
                ]))),
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            measurement_buffer.push(new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0));
            measurement_buffer.push(new_point("2025-02-10T13:19:01Z", WrappedMeasurementValue::U64(1), 0));
            measurement_buffer.push(new_point("2025-02-10T13:19:02Z", WrappedMeasurementValue::U64(2), 0));
            measurement_buffer.push(new_point("2025-02-10T13:19:03Z", WrappedMeasurementValue::U64(3), 0));

            let mut point_with_attributes = new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 0);
            point_with_attributes.add_attr("test", "unit");
            measurement_buffer.push(point_with_attributes);

            let unwanted_point = new_point("2025-02-10T13:19:00Z", WrappedMeasurementValue::U64(0), 69000);

            measurement_buffer.push(unwanted_point.clone());

            Transform::apply(&mut transform_plugin, &mut measurement_buffer, &test_tranform_context).unwrap();

            assert_eq!(measurement_buffer.len(), 1);
            assert_eq!(
                measurement_buffer_to_comparable_vec(measurement_buffer.clone())[0],
                (
                    unwanted_point.timestamp,
                    unwanted_point.value,
                    unwanted_point.metric.as_u64()
                )
            );

            measurement_buffer.clear();

            measurement_buffer.push(new_point("2025-02-10T13:19:09Z", WrappedMeasurementValue::U64(18), 0));
            measurement_buffer.push(new_point("2025-02-10T13:19:10Z", WrappedMeasurementValue::U64(5), 0));

            let mut point_with_attributes = new_point("2025-02-10T13:19:09Z", WrappedMeasurementValue::U64(15), 0);
            point_with_attributes.add_attr("test", "unit");
            measurement_buffer.push(point_with_attributes);

            Transform::apply(&mut transform_plugin, &mut measurement_buffer, &test_tranform_context).unwrap();
            assert_eq!(measurement_buffer.len(), 1);
            assert_eq!(
                measurement_buffer_to_comparable_vec(measurement_buffer.clone())[0],
                (
                    timestamp_from_rfc3339("2025-02-10T13:19:00Z"),
                    WrappedMeasurementValue::U64(24),
                    4
                )
            );

            assert_eq!(transform_plugin.internal_buffer.len(), 2);
            assert!(transform_plugin.internal_buffer.contains_key(&(
                RawMetricId::from_u64(0),
                ResourceConsumer::LocalMachine,
                Resource::LocalMachine,
                vec!(("test".to_string(), AttributeValue::Str("unit")))
            )));
            assert!(transform_plugin.internal_buffer.contains_key(&(
                RawMetricId::from_u64(0),
                ResourceConsumer::LocalMachine,
                Resource::LocalMachine,
                Vec::<(String, AttributeValue)>::new()
            )));

            assert_eq!(
                transform_plugin
                    .internal_buffer
                    .get(&(
                        RawMetricId::from_u64(0),
                        ResourceConsumer::LocalMachine,
                        Resource::LocalMachine,
                        vec!(("test".to_string(), AttributeValue::Str("unit")))
                    ))
                    .unwrap()
                    .len(),
                2
            );

            assert_eq!(
                transform_plugin
                    .internal_buffer
                    .get(&(
                        RawMetricId::from_u64(0),
                        ResourceConsumer::LocalMachine,
                        Resource::LocalMachine,
                        Vec::<(String, AttributeValue)>::new()
                    ))
                    .unwrap()
                    .len(),
                1
            );
        }

        #[test]
        #[should_panic]
        fn metric_correspondence_table_lock_poisoned() {
            let mut transform_plugin = AggregationTransform::new(
                Duration::from_secs(10),
                aggregations::Function::Mean,
                Arc::new(RwLock::new(HashMap::<RawMetricId, RawMetricId>::from([
                    (RawMetricId::from_u64(1), RawMetricId::from_u64(4)),
                    (RawMetricId::from_u64(2), RawMetricId::from_u64(7)),
                ]))),
            );

            let mut measurement_buffer = MeasurementBuffer::new();

            let builder: Builder = Builder::new();
            let inspector = builder.inspect();
            let test_tranform_context: TransformContext = TransformContext {
                metrics: &inspector.metrics(),
            };

            let metric_correspondence_table_clone = Arc::clone(&transform_plugin.metric_correspondence_table.clone());

            let _ = thread::spawn(move || {
                let _lock = (*metric_correspondence_table_clone).write().unwrap();
                panic!(); // The metric_correspondence_table gets poisoned by this panic.
            })
            .join();

            let metric_correspondence_table_clone = Arc::clone(&transform_plugin.metric_correspondence_table.clone());
            assert!((*metric_correspondence_table_clone).is_poisoned()); // Check that the lock is poisoned.

            let _ = Transform::apply(&mut transform_plugin, &mut measurement_buffer, &test_tranform_context);
        }
    }
}
