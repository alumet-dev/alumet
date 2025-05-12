use std::{collections::HashMap, hash::Hash};

use fxhash::{FxBuildHasher, FxHashMap, FxHasher};

use crate::measurement::{MeasurementBuffer, MeasurementPoint};

use super::{interpolate::InterpolationReference, Timeseries};

pub struct GroupedBuffer<K: Key> {
    groups: FxHashMap<K, Timeseries>,
}

pub trait Key: Eq + Hash + Clone {
    fn new(p: &MeasurementPoint) -> Self;
}

impl<K: Key> GroupedBuffer<K> {
    pub fn extend(&mut self, buf: &MeasurementBuffer) {
        for p in buf {
            let key = K::new(p);
            self.groups
                .entry(key)
                .or_insert_with(|| Timeseries { points: Vec::new() })
                .points
                .push(p.to_owned());
        }
    }

    pub fn get(&self, key: &K) -> Option<&Timeseries> {
        self.groups.get(key)
    }

    pub fn extract_common_range(&mut self, around_ref: &K) -> Option<i32> {
        let mut indices: FxHashMap<K, usize> =
            HashMap::with_capacity_and_hasher(self.groups.len(), FxBuildHasher::new());
        // initialization: start at the end of each timeseries
        for (k, v) in &self.groups {
            if v.points.len() == 0 {
                return None;
            }
            indices.insert(k.to_owned(), v.points.len() - 1);
        }

        // start at the end of the reference timeseries
        let mut ref_series = self.groups.get(around_ref).unwrap();
        let mut ref_i = indices.get(around_ref).unwrap();
        let mut ref_t = ref_series.points.get(*ref_i).unwrap().timestamp;

        // adjust the reference point
        // find positions in each non-ref timeseries where we have (t_a, t_b) with t_a <= t_ref <= t_b

        None
    }
}
