use core::f64;
use std::{
    collections::HashMap,
    mem::Discriminant,
    time::{SystemTime, UNIX_EPOCH},
};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    metrics::RawMetricId,
    pipeline::{
        elements::{error::TransformError, transform::TransformContext},
        Transform,
    },
    resources::{Resource, ResourceConsumer},
    timeseries::grouped_buffer::GroupedBuffer,
};
use evalexpr::{ContextWithMutableFunctions, ContextWithMutableVariables, Function};

use crate::formula::Formula;

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

pub struct FormulaInput {
    metric: RawMetricId,
    attributes_filter: Vec<(String, String)>,
}
