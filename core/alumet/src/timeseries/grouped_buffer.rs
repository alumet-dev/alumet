use std::{hash::Hash, ops::RangeInclusive};

use rustc_hash::FxHashMap;

use crate::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp},
    timeseries::{interpolate::Interpolated, together::Together},
};

use super::{Timeseries, interpolate::InterpolationReference};

pub struct GroupedBuffer<K: Key> {
    groups: FxHashMap<K, Timeseries>,
}

pub trait Key: Eq + Hash + Clone {}

pub trait KeyFactory: Clone {
    type Key: Key;

    /// Computes the key of one measurement.
    fn new_key(&mut self, p: &MeasurementPoint) -> Self::Key;
}

impl<K: Key> GroupedBuffer<K> {
    pub fn extend(
        &mut self,
        buf: &MeasurementBuffer,
        filter: impl Fn(&MeasurementPoint) -> bool,
        key_factory: &mut impl KeyFactory<Key = K>,
    ) {
        for p in buf {
            if !filter(p) {
                continue;
            }

            let key = key_factory.new_key(p);
            self.groups
                .entry(key)
                .or_insert_with(Default::default)
                .points
                .push(p.to_owned()); // TODO avoid copy
        }
    }

    pub fn push(&mut self, key: K, p: MeasurementPoint) {
        self.groups.entry(key).or_insert_with(Default::default).points.push(p);
    }

    pub fn get(&self, key: &K) -> Option<&Timeseries> {
        self.groups.get(key)
    }

    pub fn extract_common_range(&mut self, temporal_reference_key: &K) -> Option<RangeInclusive<Timestamp>> {
        let ref_series = self.groups.remove(temporal_reference_key).unwrap();
        assert!(!self.groups.is_empty());
        // inf = max_S(min_t(S))
        // sup = min_S(max_t(S))
        // ref_first = min_t(t(ref) | t >= inf)
        // ref_last = min_t(t(ref) | t <= sup)
        // range = [ref_first, ref_last]
        let inf = self
            .groups
            .values()
            .map(|series| series.first().unwrap().timestamp)
            .max()
            .unwrap();
        let sup = self
            .groups
            .values()
            .map(|series| series.last().unwrap().timestamp)
            .min()
            .unwrap();
        let ref_first = ref_series
            .points
            .iter()
            .map(|p| p.timestamp)
            .filter(|t| t >= &inf)
            .next();
        let ref_last = ref_series
            .points
            .iter()
            .rev()
            .map(|p| p.timestamp)
            .filter(|t| t <= &sup)
            .next();
        self.groups.insert(temporal_reference_key.to_owned(), ref_series);
        if let (Some(a), Some(b)) = (ref_first, ref_last) {
            Some(RangeInclusive::new(a, b))
        } else {
            None
        }
    }

    pub fn interpolate_all(mut self, temporal_reference_key: &K) -> Option<Together<Interpolated<MeasurementPoint>>> {
        // interpolate each non-reference timeseries at reference timestamps
        let range = self.extract_common_range(temporal_reference_key)?;
        let ref_series = self.groups.remove(temporal_reference_key).unwrap();
        let interp_time = InterpolationReference::from(ref_series.as_slice().restrict(range));
        let res = self
            .groups
            .into_values()
            .map(|s| s.interpolate_linear(interp_time.clone()))
            .collect();
        Some(Together::new(res))
    }
}
