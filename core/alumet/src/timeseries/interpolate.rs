use std::cmp::Ordering;

use super::Timeseries;
use crate::{
    measurement::{MeasurementPoint, Timestamp, WrappedMeasurementValue},
    timeseries::Timeslice,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterpolationReference {
    /// Timestamp to interpolate at, sorted.
    t: Vec<Timestamp>,
}

impl From<Vec<Timestamp>> for InterpolationReference {
    fn from(value: Vec<Timestamp>) -> Self {
        assert!(value.is_sorted());
        Self { t: value }
    }
}

impl From<Timeslice<'_>> for InterpolationReference {
    fn from(value: Timeslice) -> Self {
        Self {
            t: value.points.iter().map(|p| p.timestamp).collect(),
        }
    }
}

enum PointSearchResult<'a> {
    At(usize, &'a MeasurementPoint),
    Around {
        before: (usize, &'a MeasurementPoint),
        after: (usize, &'a MeasurementPoint),
    },
    NotFound,
}

/// Interpolates two measurements.
pub trait Interpolator2 {
    fn interpolate(&self, t: &Timestamp, before: &MeasurementPoint, after: &MeasurementPoint) -> MeasurementPoint;
}

pub struct LinearInterpolator;

impl Interpolator2 for LinearInterpolator {
    fn interpolate(&self, t: &Timestamp, before: &MeasurementPoint, after: &MeasurementPoint) -> MeasurementPoint {
        let x = before.value.as_f64();
        let y = after.value.as_f64();
        let t_x = &before.timestamp;
        let t_y = &after.timestamp;
        // TODO convert everything to nanoseconds? Or find a nicer representation of the timestamp (microseconds maybe?)
        // let u = (t.0-t_x.0)/(t_y.0-t_x.0);
        let u = (t.duration_since(*t_x).unwrap().as_secs_f64()) / (t_y.duration_since(*t_x).unwrap().as_secs_f64());
        let interpolated = (1.0 - u) * x + u * y;

        // TODO how to handle the other fields and the attributes?
        // Create a point with the same fields and attributes as `before`, but the interpolated value and time.
        let mut point = before.clone();
        point.timestamp = *t;
        // get a value of the expected type
        point.value = match point.value {
            WrappedMeasurementValue::F64(_) => WrappedMeasurementValue::F64(interpolated),
            WrappedMeasurementValue::U64(_) => WrappedMeasurementValue::U64(interpolated.round() as u64),
        };
        point
    }
}

impl Timeseries {
    pub fn interpolate_at(
        &self,
        interp_ref: &InterpolationReference,
        interpolator: impl Interpolator2,
    ) -> Vec<Interpolated<MeasurementPoint>> {
        // TODO this could be turned into an iterator!
        let mut res = Vec::with_capacity(interp_ref.t.len());

        let mut search_start = 0;
        for t_ref in interp_ref.t.iter() {
            match self.find_points_around(search_start, t_ref) {
                PointSearchResult::At(i, p) => {
                    // nothing to interpolate, take the point as it is
                    res.push(Interpolated::Value(p.to_owned()));

                    // The current point could be used as a "before" next time, keep it.
                    search_start = i;
                }
                PointSearchResult::Around { before, after } => {
                    // create a point in between these two
                    let new_point = interpolator.interpolate(t_ref, before.1, after.1);
                    res.push(Interpolated::Value(new_point));

                    // The current "after" could become the next "before".
                    // It is also possible that current "before" is reused next time, if there are no other point in the timeseries that is before the next t_ref.
                    // Therefore, we must search again from **before** next time.
                    search_start = before.0;
                }
                PointSearchResult::NotFound => {
                    // cannot interpolate here
                    res.push(Interpolated::Missing(*t_ref));
                }
            }
        }
        res
    }

    fn find_points_around(&'_ self, start_index: usize, t: &Timestamp) -> PointSearchResult<'_> {
        // TODO support keeping B > 1 points before and A > 1 points after
        let mut before = None;
        let mut after = None;

        // find the points that are just before t and just after t, or at exactly t
        for i in start_index..self.points.len() {
            // TODO optimize
            let p = &self.points[i];
            match p.timestamp.cmp(t) {
                Ordering::Less => before = Some((i, p)),
                Ordering::Equal => return PointSearchResult::At(i, p),
                Ordering::Greater => {
                    after = Some((i, p));
                    break;
                }
            }
        }

        if let (Some(before), Some(after)) = (before, after) {
            PointSearchResult::Around { before, after }
        } else {
            PointSearchResult::NotFound
        }
    }
}

impl InterpolationReference {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn extract_range(
        self,
        t_min: &Timestamp,
        t_max: &Timestamp,
    ) -> (InterpolationReference, InterpolationReference, InterpolationReference) {
        let first_after_min = self.t.iter().enumerate().find(|(_i, t)| t >= &t_min).map(|(i, _t)| i);
        let last_before_max = self
            .t
            .iter()
            .enumerate()
            .rev()
            .find(|(_i, t)| t <= &t_max)
            .map(|(i, _t)| i);

        match (first_after_min, last_before_max) {
            (None, _) => {
                // every timestamp is before t_min
                (self, Self::empty(), Self::empty())
            }
            (_, None) => {
                // every timestamp is after t_max
                (Self::empty(), Self::empty(), self)
            }
            (Some(first), Some(last)) => {
                // extract [0..first[, [first..=last], ]last..]
                fn split_vec<T>(mut v: Vec<T>, at: usize) -> (Vec<T>, Vec<T>) {
                    let right = v.split_off(at);
                    let left = v;
                    (left, right)
                }

                // before = [0..first[, not_before = [first..]
                let (before, not_before) = split_vec(self.t, first);

                // in_range = [first..last], after = [last+1..]
                // [0 1 2 3 4 5 6]
                //    ^       ^
                //    f       l
                let last_shifted = last - first;
                let (in_range, after) = split_vec(not_before, last_shifted + 1);
                log::trace!("first={first}, last={last}, last_shifted={last_shifted}");

                (Self::from(before), Self::from(in_range), Self::from(after))
            }
        }
    }
}

#[derive(Debug)]
pub enum Interpolated<A> {
    /// Interpolated value.
    Value(A),
    /// Not enough data to interpolate at this timestamp.
    Missing(Timestamp),
}

impl<A> Interpolated<A> {
    pub fn unwrap(&self) -> &A {
        match self {
            Interpolated::Value(v) => v,
            Interpolated::Missing(t) => panic!("Interpolated result does not contain any value at {t:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::time::{Duration, SystemTime};

    use crate::{
        measurement::{MeasurementPoint, Timestamp, WrappedMeasurementValue},
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
        timeseries::interpolate::LinearInterpolator,
    };

    use super::{Interpolated, InterpolationReference, Timeseries};

    fn t_epoch_secs(seconds: u64) -> Timestamp {
        Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
    }

    fn t_epoch_secs_millis(seconds: u64, ms: u16) -> Timestamp {
        Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_millis(ms as u64))
    }

    fn value_u64(p: &Interpolated<MeasurementPoint>) -> u64 {
        match p.unwrap().value {
            WrappedMeasurementValue::U64(v) => v,
            _ => panic!("unexpected value type for point {p:?}"),
        }
    }

    fn value_f64(p: &Interpolated<MeasurementPoint>) -> f64 {
        match p.unwrap().value {
            WrappedMeasurementValue::F64(v) => v,
            _ => panic!("unexpected value type for point {p:?}"),
        }
    }

    #[test]
    fn linterp_2_points() {
        let metric = RawMetricId(0);
        let t0 = Timestamp::from(SystemTime::UNIX_EPOCH);
        let t1 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(1));
        let points = vec![data_point(t0, metric, 0.0), data_point(t1, metric, 1.0)];

        let series = Timeseries::from(points);

        // interpolate at borders
        let t_ref = InterpolationReference::from(vec![t0]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(1.0)),
            "wrong interpolation result {interpolated:?}"
        );

        // interpolate somewhere between t0 and t1
        let t_ref = InterpolationReference::from(vec![Timestamp::from(
            SystemTime::UNIX_EPOCH + Duration::from_secs_f64(0.5),
        )]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.5)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![Timestamp::from(
            SystemTime::UNIX_EPOCH + Duration::from_secs_f64(0.25),
        )]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.25)),
            "wrong interpolation result {interpolated:?}"
        );
    }

    #[test]
    fn linterp_3_points() {
        let metric = RawMetricId(0);
        let t0 = Timestamp::from(SystemTime::UNIX_EPOCH);
        let t1 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(1));
        let t2 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(2));
        let points = vec![
            data_point(t0, metric, 0.0),
            data_point(t1, metric, 1.0),
            data_point(t2, metric, 2.0),
        ];

        let series = Timeseries::from(points);

        // interpolate at reference points
        let t_ref = InterpolationReference::from(vec![t0]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(1.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t2]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(2.0)),
            "wrong interpolation result {interpolated:?}"
        );

        // interpolate in between
        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_at(&t_ref, LinearInterpolator);
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(1.0)),
            "wrong interpolation result {interpolated:?}"
        );
    }

    #[test]
    fn linterp_high_freq_ref_low_freq_var() {
        let metric = RawMetricId(0);
        let reference = (0..=10).map(t_epoch_secs).collect::<Vec<_>>();
        let reference = InterpolationReference::from(reference);
        let points = vec![
            data_point(t_epoch_secs(0), metric, 0.0),
            data_point(t_epoch_secs(10), metric, 100.0),
        ];

        let interpolated = Timeseries::from(points).interpolate_at(&reference, LinearInterpolator);
        println!("interpolated: {interpolated:?}");
        check_interpolated_timestamps(&interpolated, &reference);
        assert_eq!(value_f64(&interpolated[0]), 0.0);
        assert_eq!(value_f64(&interpolated[1]), 10.0);
        assert_eq!(value_f64(&interpolated[2]), 20.0);
        assert_eq!(value_f64(&interpolated[3]), 30.0);
        assert_eq!(value_f64(&interpolated[4]), 40.0);
        assert_eq!(value_f64(&interpolated[5]), 50.0);
        assert_eq!(value_f64(&interpolated[6]), 60.0);
        assert_eq!(value_f64(&interpolated[7]), 70.0);
        assert_eq!(value_f64(&interpolated[8]), 80.0);
        assert_eq!(value_f64(&interpolated[9]), 90.0);
        assert_eq!(value_f64(&interpolated[10]), 100.0);
    }

    #[test]
    fn linterp_low_freq_ref_high_freq_var() {
        let metric = RawMetricId(0);
        let reference =
            InterpolationReference::from(vec![t_epoch_secs(0), t_epoch_secs_millis(5, 500), t_epoch_secs(10)]);
        let points = (0..=10)
            .map(|t| data_point(t_epoch_secs(t), metric, t as f64))
            .collect::<Vec<_>>();

        let interpolated = Timeseries::from(points).interpolate_at(&reference, LinearInterpolator);
        println!("interpolated: {interpolated:?}");
        check_interpolated_timestamps(&interpolated, &reference);
        assert_eq!(value_f64(&interpolated[0]), 0.0);
        assert_eq!(value_f64(&interpolated[1]), 5.5); // values around: 5 and 6 => interp at 5.5 in the middle
        assert_eq!(value_f64(&interpolated[2]), 10.0);
    }

    fn check_interpolated_timestamps(
        interpolated: &Vec<Interpolated<MeasurementPoint>>,
        t_ref: &InterpolationReference,
    ) {
        let t_interp: Vec<Timestamp> = interpolated
            .iter()
            .map(|i| match i {
                Interpolated::Value(point) => point.timestamp,
                Interpolated::Missing(timestamp) => *timestamp,
            })
            .collect();
        let t_ref = &t_ref.t;
        assert_eq!(
            &t_interp, t_ref,
            "wrong interpolation timestamps in result {interpolated:?}"
        );
    }

    fn data_point(t: Timestamp, metric: RawMetricId, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            t,
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(value),
        )
    }
}
