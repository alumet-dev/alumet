use crate::metrics::{
    def::{Metric, RawMetricId},
    registry::MetricRegistry,
};

use super::Transform;

/// Trait for transform builders.
///
///  # Example
/// ```
/// use alumet::pipeline::elements::transform::builder::{TransformBuilder, TransformBuildContext};
/// use alumet::pipeline::Transform;
///
/// fn build_my_transform() -> anyhow::Result<Box<dyn Transform>> {
///     todo!("build a new transform")
/// }
///
/// let builder: &dyn TransformBuilder = &|ctx: &mut dyn TransformBuildContext| {
///     let transform = build_my_transform()?;
///     Ok(transform)
/// };
/// ```
pub trait TransformBuilder: FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<Box<dyn Transform>> {}
impl<F> TransformBuilder for F where F: FnOnce(&mut dyn TransformBuildContext) -> anyhow::Result<Box<dyn Transform>> {}

pub(super) struct BuildContext<'a> {
    pub(super) metrics: &'a MetricRegistry,
}

/// Context accessible when building a transform.
pub trait TransformBuildContext {
    /// Retrieves a metric by its name.
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;

    fn metrics(&self) -> &MetricRegistry;
}

impl TransformBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        self.metrics.by_name(name)
    }

    fn metrics(&self) -> &MetricRegistry {
        self.metrics
    }
}
