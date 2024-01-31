use std::fmt;
use std::error::Error;
use std::collections::HashMap;

use crate::{
    pipeline::{Output, Source, Transform}, plugin::AlumetStart, units::Unit
};

use crate::{metrics::{Metric, MetricId, MetricType}, pipeline};

pub struct Registry {
    pub metrics_by_id: HashMap<MetricId, Metric>,
    pub metrics_by_name: HashMap<String, MetricId>,
    pub sources: Vec<Box<dyn Source>>,
    pub transforms: Vec<Box<dyn Transform>>,
    pub outputs: Vec<Box<dyn Output>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            metrics_by_id: HashMap::new(),
            metrics_by_name: HashMap::new(),
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
        }
    }

    pub fn as_start_arg(&mut self) -> AlumetStart {
        AlumetStart { registry: self }
    }

    pub(crate) fn create_metric(
        &mut self,
        name: &str,
        value_type: MetricType,
        unit: Unit,
        description: &str,
    ) -> Result<MetricId, MetricCreationError> {
        let id = MetricId(self.metrics_by_id.len());
        if let Some(_name_conflict) = self.metrics_by_name.get(name) {
            return Err(MetricCreationError::new(format!(
                "A metric with this name already exist: {name}"
            )));
        }
        let m = Metric {
            id,
            name: String::from(name),
            description: String::from(description),
            value_type,
            unit,
        };
        self.metrics_by_name.insert(String::from(name), id);
        self.metrics_by_id.insert(id, m);
        Ok(id)
    }

    pub(crate) fn add_source(&mut self, source: Box<dyn pipeline::Source>) {
        self.sources.push(source);
    }

    pub(crate) fn add_transform(&mut self, transform: Box<dyn pipeline::Transform>) {
        self.transforms.push(transform);
    }

    pub(crate) fn add_output(&mut self, output: Box<dyn pipeline::Output>) {
        self.outputs.push(output);
    }
}

// ====== Errors ======
#[derive(Debug)]
pub struct MetricCreationError {
    pub key: String,
}

impl MetricCreationError {
    pub fn new(metric_name: String) -> MetricCreationError {
        MetricCreationError { key: metric_name }
    }
}

impl Error for MetricCreationError {}

impl fmt::Display for MetricCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "This metric has already been registered: {}", self.key)
    }
}
