use alumet::{
    measurement::{AttributeValue, MeasurementPoint, WrappedMeasurementValue},
    metrics::{RawMetricId, TypedMetricId, registry::MetricRegistry},
};
use anyhow::{Context, anyhow};
use evalexpr::{ContextWithMutableVariables, HashMapContext, Node};
use rustc_hash::FxHashMap;

pub struct PreparedFormula {
    /// Metric id of the value produced by the formula.
    pub result_metric_id: TypedMetricId<f64>,

    /// metric id -> eval variable identifier.
    pub metric_to_ident: FxHashMap<RawMetricId, String>,

    /// Formula evaluation context.
    pub eval_ctx: HashMapContext,

    /// Formula expression, precompiled.
    pub expr: Node,
}

pub struct AttributionParams {
    // filters for every metric that we want
    pub data_filters: FxHashMap<RawMetricId, Box<dyn DataFilter>>,

    // different kinds of metrics
    pub general_metrics: Vec<RawMetricId>,
    pub consumer_metrics: Vec<RawMetricId>,
    pub temporal_ref_metric: RawMetricId,
}

pub trait DataFilter: Send + 'static {
    fn accept_point(&self, point: &MeasurementPoint) -> bool;
    fn copy_attributes(&self, point: &MeasurementPoint, dst: &mut Vec<(String, AttributeValue)>);
}

impl DataFilter for super::config::FilterConfig {
    fn accept_point(&self, point: &MeasurementPoint) -> bool {
        if let Some(resource_kind) = &self.resource_kind {
            if point.resource.kind() != resource_kind {
                return false;
            }
        }
        if let Some(resource_id) = &self.resource_id {
            if point.resource.id_display().to_string() != *resource_id {
                return false;
            }
        }
        if let Some(consumer_kind) = &self.consumer_kind {
            if point.consumer.kind() != consumer_kind {
                return false;
            }
        }
        if let Some(consumer_id) = &self.consumer_id {
            if point.consumer.id_display().to_string() != *consumer_id {
                return false;
            }
        }
        for (k, v) in &self.attributes {
            let point_attr = point.attributes().find(|(k2, _)| k2 == k);
            match point_attr {
                Some((_, v2)) if v.matches(v2) => (),
                _ => return false,
            }
        }

        true
    }

    fn copy_attributes(&self, point: &MeasurementPoint, dst: &mut Vec<(String, AttributeValue)>) {
        if self.keep_attributes {
            // TODO a list of attributes to keep?
            dst.extend(
                point
                    .attributes()
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned())),
            );
        }
    }
}

pub fn prepare(
    config: super::config::FormulaConfig,
    metrics: &MetricRegistry,
    result_metric_id: TypedMetricId<f64>,
) -> anyhow::Result<(PreparedFormula, AttributionParams)> {
    let mut metric_to_ident = FxHashMap::default();
    let mut data_filters = FxHashMap::default();
    let mut general_metrics = Vec::default();
    let mut consumer_metrics = Vec::default();
    let mut temporal_ref_metric = None;

    log::debug!("preparing: {config:?}");

    // Gather the MetricId of the metrics that are used in the formula.
    for (ident, serie_config) in config.per_resource {
        let metric_name = &serie_config.metric;
        let metric_id = metrics
            .by_name(metric_name)
            .with_context(|| format!("could not find metric '{metric_name}' for per_resource formula input '{ident}'"))?
            .0;

        // Is this the temporal reference? Save its id.
        if ident == config.reference_ident {
            temporal_ref_metric = Some(metric_id);
        }

        metric_to_ident.insert(metric_id, ident);

        general_metrics.push(metric_id);
        data_filters.insert(metric_id, Box::new(serie_config.filters) as _);
    }
    for (ident, serie_config) in config.per_consumer {
        let metric_name = &serie_config.metric;
        let metric_id = metrics
            .by_name(metric_name)
            .with_context(|| format!("could not find metric '{metric_name}' for per_consumer formula input '{ident}'"))?
            .0;
        metric_to_ident.insert(metric_id, ident);

        consumer_metrics.push(metric_id);
        data_filters.insert(metric_id, Box::new(serie_config.filters) as _);
    }

    // ensure that we have found the reference
    let temporal_ref_metric = temporal_ref_metric.with_context(|| {
        format!(
            "temporal reference '{}' not found, it should be declared in the `per_resource` timeseries",
            config.reference_ident
        )
    })?;

    // compile the expression to speed up evaluation later
    let expr = evalexpr::build_operator_tree(&config.expr)
        .with_context(|| format!("failed to compile expression {}", config.expr))?;

    let formula = PreparedFormula {
        result_metric_id,
        metric_to_ident,
        eval_ctx: evalexpr::HashMapContext::new(),
        expr,
    };
    let params = AttributionParams {
        data_filters,
        general_metrics,
        consumer_metrics,
        temporal_ref_metric,
    };

    Ok((formula, params))
}

impl PreparedFormula {
    pub fn evaluate(&mut self, multi_point: &FxHashMap<RawMetricId, MeasurementPoint>) -> anyhow::Result<f64> {
        // prepare the environment
        self.eval_ctx.clear_variables();
        for (k, p) in multi_point {
            let ident = self.metric_to_ident.get(&k).unwrap().to_owned();
            let value = convert_value_for_eval(&p.value);
            self.eval_ctx.set_value(ident, value).unwrap();
        }

        // evaluate
        let res = self.expr.eval_with_context(&self.eval_ctx)?;
        match res {
            evalexpr::Value::Float(v) => Ok(v),
            evalexpr::Value::Int(v) => {
                // we only support floats for now (the result metric is created as f64), convert
                Ok(v as f64)
            }
            wrong => Err(anyhow!(
                "invalid value produced by the formula: expected int or float, got {wrong:?}"
            )),
        }
    }
}

fn convert_value_for_eval(value: &WrappedMeasurementValue) -> evalexpr::Value {
    match value {
        WrappedMeasurementValue::F64(v) => evalexpr::Value::Float(*v),
        WrappedMeasurementValue::U64(v) => evalexpr::Value::Int(
            v.to_owned()
                .try_into()
                .expect("point value exceeded the maximum integer value supported by evalexpr"),
        ),
    }
}
