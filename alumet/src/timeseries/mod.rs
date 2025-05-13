use std::hash::Hash;

use env_logger::fmt::Timestamp;
use fxhash::FxHashMap;

use crate::{
    measurement::MeasurementPoint,
    metrics::RawMetricId,
    resources::{Resource, ResourceConsumer},
};

pub mod grouped_buffer;
pub mod interpolate;
pub mod window;

#[derive(Default)]
pub struct Timeseries {
    // **sorted** (by timestamp) points
    points: Vec<MeasurementPoint>,
}

pub struct GroupedTimeseries<K: Eq + Hash> {
    groups: FxHashMap<K, Timeseries>,
}

#[derive(PartialEq, Eq, Hash)]
pub struct GroupKey(RawMetricId, Resource, ResourceConsumer);

impl Timeseries {
    pub fn group(self) -> GroupedTimeseries<GroupKey> {
        // TODO opt: reuse buffers across grouping operations
        let mut groups: FxHashMap<GroupKey, Timeseries> = FxHashMap::default();
        for p in self.points.into_iter() {
            let key = GroupKey(p.metric, p.resource.clone(), p.consumer.clone());
            groups
                .entry(key)
                .and_modify(|series| series.points.push(p))
                .or_insert_with(|| Timeseries { points: Vec::new() });
        }
        GroupedTimeseries { groups }
    }

    pub fn first(&self) -> Option<&MeasurementPoint> {
        self.points.first()
    }

    pub fn last(&self) -> Option<&MeasurementPoint> {
        self.points.last()
    }
}
