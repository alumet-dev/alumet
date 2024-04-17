//! Registry of pipeline elements.

use super::runtime::{ConfiguredOutput, ConfiguredTransform};
use crate::{metrics::RawMetricId, pipeline};

/// A registry of pipeline elements: [`pipeline::Source`], [`pipeline::Transform`] and [`pipeline::Output`].
///
/// New elements are registered by the plugins during their initialization.
/// To do so, they use the methods provided by [`alumet::plugin::AlumetStart`](crate::plugin::AlumetStart).
pub struct ElementRegistry {
    pub(crate) sources: Vec<(Box<dyn pipeline::Source>, String)>,
    pub(crate) transforms: Vec<pipeline::runtime::ConfiguredTransform>,
    pub(crate) outputs: Vec<pipeline::runtime::ConfiguredOutput>,

    // Channel: outputs -> listeners of late metric registration
    pub(crate) late_reg_res_tx: tokio::sync::mpsc::Sender<Vec<RawMetricId>>,
    pub(crate) late_reg_res_rx: tokio::sync::mpsc::Receiver<Vec<RawMetricId>>,
}

impl ElementRegistry {
    pub(crate) fn new() -> Self {
        let (late_reg_res_tx, late_reg_res_rx) = tokio::sync::mpsc::channel::<Vec<RawMetricId>>(256);
        ElementRegistry {
            sources: Vec::new(),
            transforms: Vec::new(),
            outputs: Vec::new(),
            late_reg_res_rx,
            late_reg_res_tx,
        }
    }

    /// Returns the total number of sources in the registry (all plugins included).
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Returns the total number of transforms in the registry (all plugins included).
    pub fn transform_count(&self) -> usize {
        self.transforms.len()
    }

    /// Returns the total number of outputs in the registry (all plugins included).
    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    pub(crate) fn add_source(&mut self, plugin_name: String, source: Box<dyn pipeline::Source>) {
        self.sources.push((source, plugin_name));
    }

    pub(crate) fn add_transform(&mut self, plugin_name: String, transform: Box<dyn pipeline::Transform>) {
        self.transforms.push(ConfiguredTransform { transform, plugin_name });
    }

    pub(crate) fn add_output(&mut self, plugin_name: String, output: Box<dyn pipeline::Output>) {
        self.outputs.push(ConfiguredOutput { output, plugin_name });
    }
}
