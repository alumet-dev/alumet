use alumet::{
    measurement::{MeasurementBuffer, MeasurementPoint, Timestamp},
    metrics::RawMetricId,
    pipeline::{
        Transform,
        elements::{error::TransformError, transform::TransformContext},
    },
    resources::{Resource, ResourceConsumer},
    timeseries::multi_interp::MultiSyncInterpolator,
};
use rustc_hash::{FxBuildHasher, FxHashMap};

use super::prepared::{AttributionParams, PreparedFormula};

pub struct GenericAttributionTransform {
    state: AttributionState,
    formula: PreparedFormula,
}

impl GenericAttributionTransform {
    pub fn new(formula: PreparedFormula, params: AttributionParams) -> Self {
        Self {
            state: AttributionState {
                buffer_per_resource: FxHashMap::default(),
                params,
            },
            formula,
        }
    }
}

pub struct AttributionState {
    // Why not use (Resource, ResourceConsumer) as the key?
    // Because we want to easily obtain the list of consumers for each resource.
    buffer_per_resource: FxHashMap<Resource, ResourceData>,

    params: AttributionParams,
}

#[derive(Debug, Default)]
pub struct ResourceData {
    general: ByMetricBuffer,
    per_consumer: FxHashMap<ResourceConsumer, ByMetricBuffer>,
    // TODO support more complex keys
}

#[derive(Debug, Default)]
struct ByMetricBuffer(FxHashMap<RawMetricId, Vec<MeasurementPoint>>);

impl ByMetricBuffer {
    fn push(&mut self, p: MeasurementPoint) {
        self.0.entry(p.metric).or_default().push(p);
    }

    fn remove_before(&mut self, before_excl: &Timestamp) {
        for buf in self.0.values_mut() {
            let i_first_ok = buf
                .iter()
                .enumerate()
                .find_map(|(i, m)| if &m.timestamp < before_excl { None } else { Some(i) });
            if let Some(end) = i_first_ok {
                buf.drain(..end);
            }
        }
    }
}

impl AttributionState {
    fn extend(&mut self, buf: &MeasurementBuffer) {
        for p in buf {
            let filter = self.params.data_filters.get(&p.metric);
            if !filter.is_some_and(|f| f.accept_point(p)) {
                // we don't need this data point
                log::trace!("filtered out: {p:?}");
                continue;
            }

            let is_general = self.params.general_metrics.contains(&p.metric);
            let is_per_consumer = self.params.consumer_metrics.contains(&p.metric);

            let data = self.buffer_per_resource.entry(p.resource.clone()).or_default();

            if is_general {
                data.general.push(p.to_owned());
            } else if is_per_consumer {
                data.per_consumer
                    .entry(p.consumer.clone())
                    .or_default()
                    .push(p.to_owned());
            } else {
                unreachable!();
            }
        }
    }
}

impl Transform for GenericAttributionTransform {
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        // TODO buffer more and apply the transformation by "chunk"
        // To do this properly, we need a timeout, which could work by using an async transform triggered by either the timeout or a new message.

        self.state.extend(&measurements);
        log::trace!("attribution buffer: {:?}", self.state.buffer_per_resource);

        let temporal_ref_metric = self.state.params.temporal_ref_metric;

        // for each resource
        for (resource, rd) in &mut self.state.buffer_per_resource {
            let general = &rd.general;

            // For now, the time reference MUST be a "general" per-resource metric.
            // However, it may not be present in the buffer if we have not received it yet.
            let temporal_ref = general.0.get(&temporal_ref_metric);

            if temporal_ref.is_none() {
                // todo write this nicely
                continue;
            }
            let temporal_ref = temporal_ref.unwrap();

            // for each consumer of this resource
            for (consumer, cd) in &mut rd.per_consumer {
                // build a map K -> timeseries
                // with K = RawMetricId
                // and timeseries = (general points) U (per_consumer points)
                let mut series = FxHashMap::with_hasher(FxBuildHasher); // TODO optimize (no need for a hashmap actually)
                for (k, points) in &general.0 {
                    series.insert(k.to_owned(), points.as_slice());
                }
                for (k, points) in &cd.0 {
                    series.insert(k.to_owned(), points.as_slice());
                }

                // prepare the timeseries synchronizer-interpolator
                let sync = MultiSyncInterpolator {
                    reference: &temporal_ref,
                    reference_key: temporal_ref_metric,
                    series,
                };

                // compute in which limits we can interpolate
                let boundaries = sync.interpolation_boundaries();
                log::trace!("interpolation boundaries for {consumer:?}: {boundaries:?}");
                if let Some(boundaries) = boundaries {
                    // we have enough data to perform an synchronisation, let's do it!
                    let synced = sync.sync_interpolate(&boundaries);

                    // for each multi-point, evaluate the attribution formula
                    for (t, multi_point) in synced.series {
                        log::trace!("evaluating formula at {t:?} with {multi_point:?}");
                        // compute the value
                        let attributed = self
                            .formula
                            .evaluate(&multi_point)
                            .map_err(TransformError::UnexpectedInput)?;

                        // create the data point
                        let mut point = MeasurementPoint::new(
                            t,
                            self.formula.result_metric_id,
                            resource.clone(),
                            consumer.clone(),
                            attributed,
                        );

                        // keep some attributes
                        let mut attrs = Vec::new();
                        for (in_metric, in_point) in multi_point {
                            let filter = self.state.params.data_filters.get(&in_metric).unwrap();
                            filter.copy_attributes(&in_point, &mut attrs);
                        }
                        point = point.with_attr_vec(attrs);

                        measurements.push(point);
                    }

                    // Remove old per-consumer data.
                    // It's only ok to remove all the points where p.t < ref_first, the others are needed for the interpolation (see diagrams).
                    cd.remove_before(&boundaries.ref_last.1);
                } else {
                    // not enough data yet
                    // TODO handle stale data: it could happen that we have some isolated measurements and we'll never get more, we should remove them after some time
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, ctx: &TransformContext) -> Result<(), TransformError> {
        log::trace!("applying one last time");
        // TODO make sure that nothing is lost
        self.apply(&mut MeasurementBuffer::new(), &ctx)?;
        Ok(())
    }
}
