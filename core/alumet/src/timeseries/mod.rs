use crate::measurement::{MeasurementBuffer, MeasurementPoint};

pub mod interpolate;
pub mod multi_interp;

#[derive(Default)]
pub struct Timeseries {
    // **sorted** (by timestamp) points
    points: Vec<MeasurementPoint>,
}

pub struct Timeslice<'a> {
    points: &'a [MeasurementPoint],
}

impl Timeseries {
    pub fn first(&self) -> Option<&MeasurementPoint> {
        self.points.first()
    }

    pub fn last(&self) -> Option<&MeasurementPoint> {
        self.points.last()
    }

    pub fn as_slice(&'_ self) -> Timeslice<'_> {
        Timeslice { points: &self.points }
    }
}

impl From<MeasurementBuffer> for Timeseries {
    fn from(value: MeasurementBuffer) -> Self {
        let mut points: Vec<MeasurementPoint> = value.into_iter().collect();
        points.sort_by_key(|p| p.timestamp);
        Self { points }
    }
}

impl From<Vec<MeasurementPoint>> for Timeseries {
    fn from(mut points: Vec<MeasurementPoint>) -> Self {
        points.sort_by_key(|p| p.timestamp);
        Self { points }
    }
}

impl<'a> From<&'a [MeasurementPoint]> for Timeslice<'a> {
    fn from(points: &'a [MeasurementPoint]) -> Self {
        assert!(points.is_sorted_by_key(|p| p.timestamp));
        Self { points }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_from_trait_implementation {
        use super::*;
        use crate::{
            measurement::Timestamp,
            metrics::{RawMetricId, TypedMetricId},
            resources::{Resource, ResourceConsumer},
        };
        use std::{marker::PhantomData, sync::LazyLock, time::Duration};

        static BASE_TIMESTAMP: LazyLock<Timestamp> = LazyLock::new(Timestamp::now);

        // Helper function to create test measurement points
        fn create_test_point(timestamp: Timestamp, id: u64, value: u64) -> MeasurementPoint {
            let metric: TypedMetricId<f64> = TypedMetricId(RawMetricId::from_u64(id), PhantomData);
            MeasurementPoint::new(
                timestamp,
                metric,
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                value as f64,
            )
        }

        #[test]
        fn timeslice_from_sorted_slice() {
            let id = 9;

            let points = vec![
                create_test_point(*BASE_TIMESTAMP, id, 100),
                create_test_point(*BASE_TIMESTAMP + Duration::from_secs(10), id, 200),
                create_test_point(*BASE_TIMESTAMP + Duration::from_secs(20), id, 300),
            ];

            let slice = Timeslice::from(points.as_slice());

            assert_eq!(slice.points.len(), 3);
            for (i, mp) in points.iter().enumerate() {
                assert_eq!(slice.points[i].timestamp, mp.timestamp);
            }
        }

        #[test]
        #[should_panic]
        fn timeslice_from_unsorted_slice() {
            let id = 10;

            let points = vec![
                create_test_point(*BASE_TIMESTAMP + Duration::from_secs(10), id, 200),
                create_test_point(*BASE_TIMESTAMP, id, 100),
            ];

            let _ = Timeslice::from(points.as_slice());
        }
    }
}
