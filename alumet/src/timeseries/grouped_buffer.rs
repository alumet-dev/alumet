use std::{collections::HashMap, hash::Hash, ops::RangeInclusive, time::SystemTime};

use fxhash::{FxBuildHasher, FxHashMap, FxHasher};

use crate::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp},
    metrics::RawMetricId,
    resources::{Resource, ResourceConsumer},
};

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
                .or_insert_with(Default::default)
                .points
                .push(p.to_owned());
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
}

// Standard possible keys (the trait can be implemented by external crate).
impl Key for RawMetricId {
    fn new(p: &MeasurementPoint) -> Self {
        p.metric
    }
}

impl Key for Resource {
    fn new(p: &MeasurementPoint) -> Self {
        p.resource.clone()
    }
}

impl Key for ResourceConsumer {
    fn new(p: &MeasurementPoint) -> Self {
        p.consumer.clone()
    }
}

impl Key for (RawMetricId, ResourceConsumer) {
    fn new(p: &MeasurementPoint) -> Self {
        (p.metric, p.consumer.clone())
    }
}
