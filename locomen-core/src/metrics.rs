use std::{marker::PhantomData, num::NonZeroUsize};

pub enum MetricType {
    F64,
    U64,
}

pub struct MetricId<V> {
    id: usize,
    _marker: PhantomData<V>
}

pub(crate) struct MetricInfo {
    name: String,
    r#type: MetricType,
}

pub struct MetricRegistry {
    metrics_by_id: Vec<MetricInfo>,
}

impl MetricRegistry {
    pub fn with_capacity(capacity: usize) -> MetricRegistry {
        MetricRegistry {
            metrics_by_id: Vec::with_capacity(capacity),
        }
    }
    fn register<V>(&mut self, metric_name: String, metric_type: MetricType) -> MetricId<V> {
        self.metrics_by_id.push(MetricInfo {
            name: metric_name,
            r#type: metric_type,
        });
        let id = self.metrics_by_id.len() - 1;
        MetricId { id, _marker: PhantomData }
    }

    pub fn register_u64(&mut self, metric_name: String) -> MetricId<u64> {
        self.register(metric_name, MetricType::U64)
    }

    pub fn register_f64(&mut self, metric_name: String) -> MetricId<u64> {
        self.register(metric_name, MetricType::F64)
    }
}

pub struct MetricBuffer {}

impl MetricBuffer {
    pub fn add<V>(&mut self, metric: &MetricId<V>, value: V, metadata: Vec<(&str, &str)>) {
        todo!()
    }
}

pub trait MetricSource {
    type Err;

    // TODO utiliser autre chose que MetricBuffer, plus restreint
    fn poll(&self, buf: &mut MetricBuffer) -> Result<(), Self::Err>;
}

pub trait MetricTransformer {
    type Err;

    // TODO utiliser autre chose que MetricBuffer, plus libre
    fn transform(&self, m: &mut MetricBuffer) -> Result<(), Self::Err>;
}

pub enum TypedMetric {
    F64{id: MetricId<f64>, value: f64},
    U64{id: MetricId<u64>, value: u64},
}

impl TypedMetric {
    pub fn metric_type(&self) -> MetricType {
        match self {
            TypedMetric::F64 { id: _, value: _ } => MetricType::F64,
            TypedMetric::U64 { id: _, value: _ } => MetricType::U64,
        }
    }
}

pub struct Metric {
    pub typed: TypedMetric,
    pub metadata: Vec<(String, String)>
}

pub struct Metric2 {
    pub typ: MetricType,
    pub id: u64,
    pub value: u64,
    pub metadata: Vec<(String, String)>
}
