use core::f64;
use std::{
    collections::HashMap,
    mem::Discriminant,
    time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
    timeseries::grouped_buffer::GroupedBuffer,
};
use evalexpr::{ContextWithMutableFunctions, ContextWithMutableVariables, Function};

pub struct EnergyAttributionTransform {
    // Ce dont on a besoin pour la nouvelle transformation configurable :
    // - la formule d'attribution
    // - les variables d'entrée, avec leur métriques, comment on doit gérer les resources, consumers et attributs
    // - quelle est la variable d'entrée de référence (on va dire que c'est l'énergie, par ex.)
    // - la variable de sortie, quelle ressource, consumer et attributs on doit lui donner
    // - la ressource principale que l'on prend en compte (pour éviter de la répéter pour chaque variable d'entrée)
    formula: Formula,
    buffers: HashMap<RawMetricId, Vec<MeasurementPoint>>,
}

pub struct Formula {
    expression: evalexpr::Node,
    reference_timeseries: RawMetricId,
    inputs: HashMap<String, RawMetricId>,
    resource_kind: Discriminant<Resource>, // todo
    resource_aggregate: AggregateOperator,
}

pub struct FormulaInput {
    metric: RawMetricId,
    attributes_filter: Vec<(String, String)>,
}

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

impl Transform for EnergyAttributionTransform {
    /// Applies the transform on the measurements.
    fn apply(&mut self, measurements: &mut MeasurementBuffer, _ctx: &TransformContext) -> Result<(), TransformError> {
        for m in measurements {
            if let Some(buf) = self.buffers.get_mut(&m.metric) {
                buf.push(m.to_owned());
            }
        }

        self.grouped_buffer.extend(measurements);
        self.grouped_buffer.extract_min_all();
        let energy = self.grouped_buffer.take(energy_metric);
        let consumption = self.grouped_buffer.take(consumption_metric);
        let consumption = consumption.interpolate(energy);
        // let result = energy*consumption

        // Retrieve the pod_id and the rapl_id.
        // Using a nested scope to reduce the lock time.
        let (hardware_usage_id, consumed_energy_id) = {
            let metrics = &self.metrics;

            let hardware_usage_id = metrics.hardware_usage.as_u64();
            let consumed_energy_id = metrics.consumed_energy.as_u64();

            (hardware_usage_id, consumed_energy_id)
        };

        // Filling the buffers.
        for m in measurements.clone().iter() {
            if m.metric.as_u64() == consumed_energy_id {
                let _ = &self.add_to_energy_buffer(m.clone())?;
            } else if m.metric.as_u64() == hardware_usage_id {
                let _ = &self.add_to_hardware_buffer(m.clone())?;
            }
        }

        // Emptying the buffers and pushing the energy attribution to the MeasurementBuffer
        self.buffer_bouncer(measurements);

        Ok(())
    }
}
