use std::{collections::HashMap, mem::Discriminant, time::Duration};

use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint},
    metrics::RawMetricId,
    resources::{Resource, ResourceConsumer},
    timeseries::{
        self,
        grouped_buffer::{GroupedBuffer, Key},
        interpolate::Interpolated,
        together::Together,
    },
};
use evalexpr::HashMapContext;
use serde::Deserialize;

pub struct Formula {
    expression: evalexpr::Node,
    metrics_mapping: HashMap<String, RawMetricId>,
    reference_serie: RawMetricId,
    // resource_kind: Discriminant<Resource>, // todo
    resource_kind: String,
    resource_aggregate: AggregateOperator,
    ref_key: AttributionKey,
    expr_uses_delta_time: bool, // TODO
}

#[derive(Debug, Deserialize)]
struct FormulaConfig {
    formula_expr: String,
    result_metric: ConfigResultMetric,
    resource: ConfigResource,
    // todo
}

#[derive(Debug, Deserialize)]
struct ConfigResultMetric {
    metric: String,
    unit: String,
    r#type: String,
}

#[derive(Debug, Deserialize)]
struct ConfigResource {
    kind: String,
    aggregate: AggregateOperator,
}

#[derive(Debug, Deserialize)]
pub enum AggregateOperator {
    Sum,
    Min,
    Max,
    Avg,
}

impl AggregateOperator {
    pub fn apply(self, values: &[f64]) -> f64 {
        match self {
            AggregateOperator::Sum => values.iter().sum(),
            AggregateOperator::Min => values.iter().cloned().reduce(f64::min).unwrap_or(f64::NAN),
            AggregateOperator::Max => values.iter().cloned().reduce(f64::max).unwrap_or(f64::NAN),
            AggregateOperator::Avg => (values.iter().sum::<f64>()) / values.len() as f64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AttributionKey {
    metric: RawMetricId,
    resource: Resource,
    consumer: ResourceConsumer,
}

impl Key for AttributionKey {
    fn new(p: &MeasurementPoint) -> Self {
        Self {
            metric: p.metric,
            resource: p.resource.clone(),
            consumer: p.consumer.clone(),
        }
    }
}

impl Formula {
    pub fn apply_to_all(&self, mut input: GroupedBuffer<AttributionKey>) {
        // assume that the input has been filtered
        // assume that we already have the energy consumption summed by package
        if let Some(interpolated) = input.interpolate_all(&self.ref_key) {
            // Which is which?

            // For each point, apply the formula.

            todo!()
        }
    }

    pub fn eval(&self, t: Together<Interpolated<MeasurementPoint>>, points: &mut Vec<MeasurementPoint>) {
        let mut ctx = HashMapContext::new();
        for interpolated in t.into_iter() {
            // if self.expr_uses_delta_time {
                // // the expression uses dt, so there needs to be a "previous" point
            // }

            // Populate the context
            ctx.clear();

            let res = self.expression.eval_with_context(&ctx).unwrap();
            let p = MeasurementPoint::new(timestamp, metric, resource, consumer, value);
            points.push(p);
        }
        // todo
    }
}
