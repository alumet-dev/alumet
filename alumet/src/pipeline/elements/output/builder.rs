use tokio::runtime;

use crate::metrics::{
    def::{Metric, RawMetricId},
    online::MetricReader,
    registry::MetricRegistry,
};

use super::{AsyncOutputStream, BoxedAsyncOutput, Output};

/// An output builder, for any type of output.
///
/// Use this type in the pipeline builder.
pub enum OutputBuilder {
    Blocking(Box<dyn BlockingOutputBuilder>),
    Async(Box<dyn AsyncOutputBuilder>),
}

/// Like [`OutputBuilder`] but with a [`Send`] bound on the builder.
///
/// Use this type in the pipeline control loop.
pub enum SendOutputBuilder {
    Blocking(Box<dyn BlockingOutputBuilder + Send>),
    Async(Box<dyn AsyncOutputBuilder + Send>),
}

impl From<SendOutputBuilder> for OutputBuilder {
    fn from(value: SendOutputBuilder) -> Self {
        match value {
            SendOutputBuilder::Blocking(b) => OutputBuilder::Blocking(b),
            SendOutputBuilder::Async(b) => OutputBuilder::Async(b),
        }
    }
}

/// Trait for builders of blocking outputs.
///
///  # Example
/// ```
/// use alumet::pipeline::elements::output::builder::{BlockingOutputBuilder, BlockingOutputBuildContext};
/// use alumet::pipeline::Output;
///
/// fn build_my_output() -> anyhow::Result<Box<dyn Output>> {
///     todo!("build a new output")
/// }
///
/// let builder: &dyn BlockingOutputBuilder = &|ctx: &mut dyn BlockingOutputBuildContext| {
///     let output = build_my_output()?;
///     Ok(output)
/// };
/// ```
pub trait BlockingOutputBuilder:
    FnOnce(&mut dyn BlockingOutputBuildContext) -> anyhow::Result<Box<dyn Output>>
{
}
impl<F> BlockingOutputBuilder for F where
    F: FnOnce(&mut dyn BlockingOutputBuildContext) -> anyhow::Result<Box<dyn Output>>
{
}

pub trait AsyncOutputBuilder:
    FnOnce(&mut dyn AsyncOutputBuildContext, AsyncOutputStream) -> anyhow::Result<BoxedAsyncOutput>
{
}
impl<F> AsyncOutputBuilder for F where
    F: FnOnce(&mut dyn AsyncOutputBuildContext, AsyncOutputStream) -> anyhow::Result<BoxedAsyncOutput>
{
}

/// Context provided when building new outputs.
pub(super) struct OutputBuildContext<'a> {
    pub(super) metrics_r: &'a MetricReader,
    pub(super) metrics: &'a MetricRegistry,
    pub(super) runtime: runtime::Handle,
}

pub trait BlockingOutputBuildContext {
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
}

pub trait AsyncOutputBuildContext {
    fn async_runtime(&self) -> &tokio::runtime::Handle;

    /// Returns a `MetricReader`, which allows to access the metric registry.
    fn metrics_reader(&self) -> MetricReader;
}

impl BlockingOutputBuildContext for OutputBuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        self.metrics.by_name(name)
    }
}

impl AsyncOutputBuildContext for OutputBuildContext<'_> {
    fn async_runtime(&self) -> &tokio::runtime::Handle {
        &self.runtime
    }

    fn metrics_reader(&self) -> MetricReader {
        self.metrics_r.clone()
    }
}
