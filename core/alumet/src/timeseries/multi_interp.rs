//! Multivariate interpolation buffer.

use std::hash::Hash;

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{
    measurement::{MeasurementPoint, Timestamp},
    timeseries::{
        Timeseries, Timeslice,
        interpolate::{Interpolated, InterpolationReference},
    },
};

/// Synchronizes multiple timeseries so that their timestamps match those of the `reference`.
///
/// Missing values are produced by linear interpolation.
pub struct MultiSyncInterpolator<'a, K: Eq + Hash + Clone> {
    /// The time reference.
    pub reference: &'a [MeasurementPoint],

    /// Key of the reference timeseries, included in the result.
    pub reference_key: K,

    /// The time series to interpolate.
    pub series: FxHashMap<K, &'a [MeasurementPoint]>,
}

pub struct InterpolationBoundaries {
    pub inf: Timestamp,
    pub sup: Timestamp,
    pub ref_first: (usize, Timestamp),
    pub ref_last: (usize, Timestamp),
}

/// Synchronization result: for each reference timestamp, the points of the timeseries (including the reference), by key.
pub struct SyncResult<K: Eq + Hash + Clone> {
    pub series: Vec<(Timestamp, FxHashMap<K, MeasurementPoint>)>,
}

impl<'a, K: Eq + Hash + Clone> MultiSyncInterpolator<'a, K> {
    /// Computes the boundaries in which all the series can be interpolated at the reference time.
    pub fn interpolation_boundaries(&self) -> Option<InterpolationBoundaries> {
        // inf = max_S(min_t(S))
        // sup = min_S(max_t(S))
        // ref_first = min_t(t(ref) | t >= inf)
        // ref_last = min_t(t(ref) | t <= sup)
        // range = [ref_first, ref_last]
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
        if let (Some(ref_first), Some(ref_last)) = (ref_first, ref_last) {
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
        let time_ref = &self.reference[ref_first..ref_last];

        // init result
        let mut res = SyncResult {
            series: Vec::with_capacity(time_ref.len()),
        };
        for ref_point in time_ref {
            // include the points of the reference
            let mut multi_points = FxHashMap::with_capacity_and_hasher(self.series.len(), FxBuildHasher);
            multi_points.insert(self.reference_key.clone(), ref_point.to_owned());
            res.series.push((ref_point.timestamp, multi_points));
        }

        // interpolate each timeserie
        for (key, serie) in self.series.iter() {
            let time = InterpolationReference::from(Timeslice::from(time_ref));
            let serie = Timeseries::from(Vec::from_iter(serie.iter().cloned())); // TODO optimize
            let interpolated = serie.interpolate_linear(time);
            // interpolated is a Vec of interpolated points,
            // push its content into a Vec of (t, multi-point)
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
