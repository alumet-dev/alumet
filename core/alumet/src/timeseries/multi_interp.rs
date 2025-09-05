//! Multivariate interpolation buffer.

use std::{fmt::Debug, hash::Hash};

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{
    measurement::{MeasurementPoint, Timestamp},
    timeseries::{
        Timeseries, Timeslice,
        interpolate::{Interpolated, InterpolationReference, LinearInterpolator},
    },
};

/// Synchronizes multiple timeseries so that their timestamps match those of the `reference`.
///
/// Missing values are produced by linear interpolation.
pub struct MultiSyncInterpolator<'a, K: Eq + Hash + Clone + Debug> {
    /// The time reference.
    pub reference: &'a [MeasurementPoint],

    /// Key of the reference timeseries, included in the result.
    pub reference_key: K,

    /// The time series to interpolate.
    pub series: &'a FxHashMap<K, &'a [MeasurementPoint]>,
}

#[derive(Debug, PartialEq)]
pub struct InterpolationBoundaries {
    /// max_S(min_t(S)): for each timeseries, find the minimum timestamp, and take the max of them
    pub inf: Timestamp,
    /// min_S(max_t(S)): for each timeseries, find the maximum timestamp, and take the min of them
    pub sup: Timestamp,
    /// The first point, in the reference series, that we can use.
    pub ref_first: (usize, Timestamp),
    /// The last point, in the reference series, that we can use.
    pub ref_last: (usize, Timestamp),
}

/// Synchronization result: for each reference timestamp, the points of the timeseries (including the reference), by key.
pub struct SyncResult<K: Eq + Hash + Clone> {
    pub series: Vec<(Timestamp, FxHashMap<K, MeasurementPoint>)>,
}

impl<'a, K: Eq + Hash + Clone + Debug> MultiSyncInterpolator<'a, K> {
    /// Computes the boundaries in which all the series can be interpolated at the reference time.
    pub fn interpolation_boundaries(&self) -> Option<InterpolationBoundaries> {
        // inf = max_S(min_t(S))
        // sup = min_S(max_t(S))
        // ref_first = min_t(t(ref) | t >= inf)
        // ref_last = max_t(t(ref) | t <= sup)
        // range = [ref_first, ref_last]
        assert!(self.reference.is_sorted_by_key(|p| p.timestamp));

        let inf = self
            .series
            .values()
            .map(|series| series.first().unwrap().timestamp)
            .max()
            .unwrap();
        let sup = self
            .series
            .values()
            .map(|series| series.last().unwrap().timestamp)
            .min()
            .unwrap();
        let ref_first = self
            .reference
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if &p.timestamp >= &inf {
                    Some((i, p.timestamp))
                } else {
                    None
                }
            })
            .next();
        let ref_last = self
            .reference
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(i, p)| {
                if &p.timestamp <= &sup {
                    Some((i, p.timestamp))
                } else {
                    None
                }
            })
            .next();
        if let (Some(ref_first), Some(ref_last)) = (ref_first, ref_last)
            && ref_first <= ref_last
        {
            Some(InterpolationBoundaries {
                inf,
                sup,
                ref_first,
                ref_last,
            })
        } else {
            None
        }
    }

    pub fn sync_interpolate(&self, boundaries: &InterpolationBoundaries) -> SyncResult<K> {
        // extract reference
        let ref_first = boundaries.ref_first.0;
        let ref_last = boundaries.ref_last.0;
        let time_ref = &self.reference[ref_first..=ref_last];

        // init result
        let mut res = SyncResult {
            series: Vec::with_capacity(time_ref.len()),
        };

        // include the points of the reference
        for ref_point in time_ref {
            let mut multi_points = FxHashMap::with_capacity_and_hasher(self.series.len(), FxBuildHasher);
            multi_points.insert(self.reference_key.clone(), ref_point.to_owned());
            res.series.push((ref_point.timestamp, multi_points));
        }

        // interpolate each timeseries
        for (key, serie) in self.series.iter() {
            let time = InterpolationReference::from(Timeslice::from(time_ref));
            let serie = Timeseries::from(Vec::from_iter(serie.iter().cloned())); // TODO optimize
            let interpolated = serie.interpolate_at(&time, LinearInterpolator);

            // merge the Vec<interpolated point> into a Vec<(t, multi-point)> with all the interpolated series
            for (i, p) in interpolated.into_iter().enumerate() {
                match p {
                    Interpolated::Value(p) => {
                        let result_to_update = &mut res.series[i];
                        assert_eq!(
                            p.timestamp, result_to_update.0,
                            "interpolation should produce timestamps that match the time reference"
                        );
                        result_to_update.1.insert(key.to_owned(), p);
                    }
                    Interpolated::Missing(timestamp) => {
                        panic!(
                            "interpolation should never produce missing values (because of the boundaries). Bad timestamp: {timestamp:?}"
                        )
                    }
                }
            }
        }

        // done
        res
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use crate::{
        measurement::WrappedMeasurementValue,
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
    };

    use super::*;

    /// Easy way to create a timestamp by adding `n_secs` seconds to the epoch.
    fn t(n_secs: u64) -> Timestamp {
        Timestamp(UNIX_EPOCH + Duration::from_secs(n_secs))
    }

    /// Creates a point with `Resource::LocalMachine` and `ResourceConsumer::LocalMachine`.
    fn point_lm(t: Timestamp, metric: RawMetricId, value: f64) -> MeasurementPoint {
        MeasurementPoint::new_untyped(
            t,
            metric,
            Resource::LocalMachine,
            ResourceConsumer::LocalMachine,
            WrappedMeasurementValue::F64(value),
        )
    }

    const METRIC_REF: RawMetricId = RawMetricId(0);
    const METRIC_A: RawMetricId = RawMetricId(1);
    const METRIC_B: RawMetricId = RawMetricId(2);
    const METRIC_C: RawMetricId = RawMetricId(3);

    /// Tests with a single non-ref variable.
    mod single_var {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn aligned_1() {
            /*
                Data points and expected result:

            ref_first = ref_last
                      v

                t     5
                ref   x
                a     x

                      ^
                  inf = sup
            */

            let data_ref = vec![point_lm(t(5), METRIC_REF, 0.0)];
            let data_a = vec![point_lm(t(5), METRIC_A, 0.0)];
            let interpolator = MultiSyncInterpolator {
                reference: &data_ref,
                reference_key: METRIC_REF,
                series: &FxHashMap::from_iter([(METRIC_A, data_a.as_slice())]),
            };
            let bounds = interpolator
                .interpolation_boundaries()
                .expect("should have valid boundaries");
            assert_eq!(
                bounds,
                InterpolationBoundaries {
                    inf: t(5),
                    sup: t(5),
                    ref_first: (0, t(5)),
                    ref_last: (0, t(5))
                }
            );

            let res = interpolator.sync_interpolate(&bounds);
            println!("res: {:?}", res.series);
            assert_eq!(
                res.series,
                vec![(
                    t(5),
                    FxHashMap::from_iter([
                        (METRIC_REF, point_lm(t(5), METRIC_REF, 0.0)),
                        (METRIC_A, point_lm(t(5), METRIC_A, 0.0)),
                    ])
                )]
            );
        }

        #[test]
        fn boundary_conditions_aligned_2() {
            /*
            Data points and expected result:

            ref_first  ref_last
                  v      v

            t     0      2
            ref   x      x
            a     x      x

                  ^      ^
                 inf    sup
            */

            let data_ref = vec![point_lm(t(0), METRIC_REF, 0.0), point_lm(t(2), METRIC_REF, 0.0)];
            let data_a = vec![point_lm(t(0), METRIC_A, 0.0), point_lm(t(2), METRIC_A, 0.0)];
            let interpolator = MultiSyncInterpolator {
                reference: &data_ref,
                reference_key: METRIC_A,
                series: &FxHashMap::from_iter([(METRIC_A, data_a.as_slice())]),
            };
            let bounds = interpolator
                .interpolation_boundaries()
                .expect("should have valid boundaries");
            assert_eq!(
                bounds,
                InterpolationBoundaries {
                    inf: t(0),
                    sup: t(2),
                    ref_first: (0, t(0)),
                    ref_last: (1, t(2))
                }
            );
        }

        #[test]
        fn boundary_conditions_aligned_3() {
            /*
            Data points:
            t     0  1  2
            ref   x  x  x
            a     x  x  x

            Expected results:
            inf is computed across all non-ref series, here we have only `a`, so it's min_t(a) = 0
            sup is computed across all non-ref series, here we have only `a`, so it's max_t(a) = 2
            ref_first is the first time in ref that is >= inf, here it's t=0 with index=0
            ref_last is the last time in ref that is <= sup, here it's t=2 with index=2
            */

            let data_ref = vec![
                point_lm(t(0), METRIC_REF, 0.0),
                point_lm(t(1), METRIC_REF, 0.0),
                point_lm(t(2), METRIC_REF, 0.0),
            ];
            let data_a = vec![
                point_lm(t(0), METRIC_A, 0.0),
                point_lm(t(1), METRIC_A, 0.0),
                point_lm(t(2), METRIC_A, 0.0),
            ];
            let interpolator = MultiSyncInterpolator {
                reference: &data_ref,
                reference_key: METRIC_A,
                series: &FxHashMap::from_iter([(METRIC_A, data_a.as_slice())]),
            };
            let bounds = interpolator
                .interpolation_boundaries()
                .expect("should have valid boundaries");
            assert_eq!(
                bounds,
                InterpolationBoundaries {
                    inf: t(0),
                    sup: t(2),
                    ref_first: (0, t(0)),
                    ref_last: (2, t(2))
                }
            );
        }

        #[test]
        fn boundary_conditions_unaligned_left() {
            /*
            Data points:
            t      0  1  2  3  4
            ref_i  0     1  2  3
            ref    x     x  x  x
            a         x  x     x
            */
            let data_ref = vec![
                point_lm(t(0), METRIC_REF, 0.0),
                point_lm(t(2), METRIC_REF, 0.0),
                point_lm(t(3), METRIC_REF, 0.0),
                point_lm(t(4), METRIC_REF, 0.0),
            ];
            let data_a = vec![
                point_lm(t(1), METRIC_A, 0.0),
                point_lm(t(2), METRIC_A, 0.0),
                point_lm(t(4), METRIC_A, 0.0),
            ];
            let interpolator = MultiSyncInterpolator {
                reference: &data_ref,
                reference_key: METRIC_A,
                series: &FxHashMap::from_iter([(METRIC_A, data_a.as_slice())]),
            };
            let bounds = interpolator
                .interpolation_boundaries()
                .expect("should have valid boundaries");
            assert_eq!(
                bounds,
                InterpolationBoundaries {
                    inf: t(1),
                    sup: t(4),
                    ref_first: (1, t(2)),
                    ref_last: (3, t(4))
                }
            );
        }

        #[test]
        fn boundary_conditions_unaligned_right() {
            /*
            Data points:
            t      0  1  2  3  4
            ref_i  0  1  2     3
            ref    x  x  x     x
            a      x     x  x
            */
            let data_ref = vec![
                point_lm(t(0), METRIC_REF, 0.0),
                point_lm(t(1), METRIC_REF, 0.0),
                point_lm(t(2), METRIC_REF, 0.0),
                point_lm(t(4), METRIC_REF, 0.0),
            ];
            let data_a = vec![
                point_lm(t(0), METRIC_A, 0.0),
                point_lm(t(2), METRIC_A, 0.0),
                point_lm(t(3), METRIC_A, 0.0),
            ];
            let interpolator = MultiSyncInterpolator {
                reference: &data_ref,
                reference_key: METRIC_A,
                series: &FxHashMap::from_iter([(METRIC_A, data_a.as_slice())]),
            };
            let bounds = interpolator
                .interpolation_boundaries()
                .expect("should have valid boundaries");
            assert_eq!(
                bounds,
                InterpolationBoundaries {
                    inf: t(0),
                    sup: t(3),
                    ref_first: (0, t(0)),
                    ref_last: (2, t(2))
                }
            );
        }
    }
}
