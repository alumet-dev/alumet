use std::collections::HashMap;

use metric::{Resource, Metric, MetricId, ResourceId};

pub mod metric;

pub struct Locomen {
    metrics: HashMap<MetricId, Metric>,
    resources: HashMap<ResourceId, Resource>,
}

impl Locomen {
    pub fn new() -> Locomen {
        Locomen { metrics: HashMap::new(), resources: HashMap::new() }
    }
}

// todo a "global" access to Locomen?