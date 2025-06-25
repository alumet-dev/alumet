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

impl Timeseries {
    pub fn interpolate_linear(&self, interp_time: InterpolationReference) -> Vec<Interpolated<MeasurementPoint>> {
        if self.points.len() < 2 {
            // not enough points to interpolate: return a vec of Interpolated::Missing values
            return interp_time.t.into_iter().map(|t| Interpolated::Missing(t)).collect();
        }

        let mut res = Vec::with_capacity(interp_time.t.len());

        let t_min = &self.points.first().unwrap().timestamp;
        let t_max = &self.points.last().unwrap().timestamp;
        let (before, in_range, after) = interp_time.extract_range(t_min, t_max);
        log::trace!("before: {before:?}\nin_range: {in_range:?}\nafter:{after:?}");

        // Add the first missing points.
        res.extend(before.t.into_iter().map(Interpolated::Missing));

        // Interpolate
        let mut i = 0;
        for t_ref in in_range.t.iter() {
            let a = &self.points[i];
            let t_a = &a.timestamp;
            log::trace!("interpolating for {t_ref:?} => i = {i}, a = {a:?}");
            if t_a == t_ref {
                res.push(Interpolated::Value(a.clone()));
                i += 1;
            } else {
                assert!(t_a < t_ref);
                while &self.points[i + 1].timestamp < t_ref {
                    i += 1;
                }
                let b = &self.points[i + 1];
                let t_b = &b.timestamp;
                // TODO convert everything to nanoseconds?
                // let u = (t_ref.0-t_a.0)/(t_b.0-t_a.0);
                let u = (t_ref.duration_since(*t_a).unwrap().as_secs_f64())
                    / (t_b.duration_since(*t_a).unwrap().as_secs_f64());

                log::trace!("interpolating for {t_ref:?} =>\n i = {i},\n a = {a:?},\n b = {b:?},\n u = {u:?}");
                // TODO configure how u64 and point's fields are handled
                let a_value = match a.value {
                    WrappedMeasurementValue::F64(v) => v,
                    WrappedMeasurementValue::U64(v) => v as f64,
                };
                let b_value = match b.value {
                    WrappedMeasurementValue::F64(v) => v,
                    WrappedMeasurementValue::U64(v) => v as f64,
                };
                let interpolated = (1.0 - u) * a_value + u * b_value;
                let mut point = a.clone();
                point.timestamp = *t_ref;
                point.value = match a.value {
                    WrappedMeasurementValue::F64(_) => WrappedMeasurementValue::F64(interpolated),
                    WrappedMeasurementValue::U64(_) => WrappedMeasurementValue::U64(interpolated.round() as u64),
                };
                res.push(Interpolated::Value(point));
            }
        }

        // Add the last missing points.
        res.extend(after.t.into_iter().map(Interpolated::Missing));

        res
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
                let (in_range, after) = split_vec(dbg!(not_before), last_shifted + 1);
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use crate::{
        measurement::{MeasurementPoint, Timestamp, WrappedMeasurementValue},
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
    };

    use super::{Interpolated, InterpolationReference, Timeseries};

    #[test]
    fn extract_range() {
        let t0 = Timestamp::from(SystemTime::UNIX_EPOCH);
        let t1 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(1));
        let t2 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(2));
        let t3 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(3));
        let t4 = Timestamp::from(SystemTime::UNIX_EPOCH + Duration::from_secs(4));

        type Ref = InterpolationReference;

        assert_eq!(
            Ref::empty().extract_range(&t0, &t0),
            (Ref::empty(), Ref::empty(), Ref::empty())
        );
        assert_eq!(
            Ref::empty().extract_range(&t1, &t1),
            (Ref::empty(), Ref::empty(), Ref::empty())
        );
        assert_eq!(
            Ref::empty().extract_range(&t0, &t1),
            (Ref::empty(), Ref::empty(), Ref::empty())
        );

        assert_eq!(
            Ref::from(vec![t0.clone()]).extract_range(&t0, &t0),
            (Ref::empty(), Ref::from(vec![t0.clone()]), Ref::empty())
        );
        assert_eq!(
            Ref::from(vec![t0.clone()]).extract_range(&t0, &t1),
            (Ref::empty(), Ref::from(vec![t0.clone()]), Ref::empty())
        );
        assert_eq!(
            Ref::from(vec![t0.clone()]).extract_range(&t1, &t1),
            (Ref::from(vec![t0.clone()]), Ref::empty(), Ref::empty())
        );

        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone()]).extract_range(&t0, &t1),
            (Ref::empty(), Ref::from(vec![t0.clone(), t1.clone()]), Ref::empty())
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone()]).extract_range(&t0, &t0),
            (Ref::empty(), Ref::from(vec![t0.clone()]), Ref::from(vec![t1.clone()]))
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone()]).extract_range(&t1, &t1),
            (Ref::from(vec![t0.clone()]), Ref::from(vec![t1.clone()]), Ref::empty())
        );

        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t0, &t3),
            (
                Ref::empty(),
                Ref::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]),
                Ref::empty()
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t1, &t3),
            (
                Ref::from(vec![t0.clone()]),
                Ref::from(vec![t1.clone(), t2.clone(), t3.clone()]),
                Ref::empty()
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t0, &t2),
            (
                Ref::empty(),
                Ref::from(vec![t0.clone(), t1.clone(), t2.clone()]),
                Ref::from(vec![t3.clone()])
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t1, &t2),
            (
                Ref::from(vec![t0.clone()]),
                Ref::from(vec![t1.clone(), t2.clone()]),
                Ref::from(vec![t3.clone()])
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t0, &t1),
            (
                Ref::empty(),
                Ref::from(vec![t0.clone(), t1.clone()]),
                Ref::from(vec![t2.clone(), t3.clone()])
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t2, &t3),
            (
                Ref::from(vec![t0.clone(), t1.clone()]),
                Ref::from(vec![t2.clone(), t3.clone()]),
                Ref::empty()
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t3, &t3),
            (
                Ref::from(vec![t0.clone(), t1.clone(), t2.clone()]),
                Ref::from(vec![t3.clone()]),
                Ref::empty()
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone()]).extract_range(&t0, &t0),
            (
                Ref::empty(),
                Ref::from(vec![t0.clone()]),
                Ref::from(vec![t1.clone(), t2.clone(), t3.clone()])
            )
        );

        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone(), t4.clone()])
                .extract_range(&t0, &t4),
            (
                Ref::empty(),
                Ref::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone(), t4.clone()]),
                Ref::empty()
            )
        );
        assert_eq!(
            InterpolationReference::from(vec![t0.clone(), t1.clone(), t2.clone(), t3.clone(), t4.clone()])
                .extract_range(&t1, &t3),
            (
                Ref::from(vec![t0.clone()]),
                Ref::from(vec![t1.clone(), t2.clone(), t3.clone()]),
                Ref::from(vec![t4.clone()])
            )
        );
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
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_linear(t_ref.clone());
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
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.5)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![Timestamp::from(
            SystemTime::UNIX_EPOCH + Duration::from_secs_f64(0.25),
        )]);
        let interpolated = series.interpolate_linear(t_ref.clone());
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
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(0.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(1.0)),
            "wrong interpolation result {interpolated:?}"
        );

        let t_ref = InterpolationReference::from(vec![t2]);
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(2.0)),
            "wrong interpolation result {interpolated:?}"
        );

        // interpolate in between
        let t_ref = InterpolationReference::from(vec![t1]);
        let interpolated = series.interpolate_linear(t_ref.clone());
        check_interpolated_timestamps(&interpolated, &t_ref);
        assert_eq!(interpolated.len(), 1);
        assert!(
            matches!(&interpolated[0], Interpolated::Value(p) if p.value == WrappedMeasurementValue::F64(1.0)),
            "wrong interpolation result {interpolated:?}"
        );
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
