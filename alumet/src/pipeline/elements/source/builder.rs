use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;

use crate::{
    measurement::MeasurementBuffer,
    metrics::{
        def::{Metric, RawMetricId},
        online::{MetricReader, MetricSender},
        registry::MetricRegistry,
    },
};

use super::interface::{AutonomousSource, Source};
use super::trigger::TriggerSpec;

// Trait aliases are unstable, and the following is not enough to help deduplicating code in plugin::phases.
//
//     pub type ManagedSourceBuilder = dyn FnOnce(params) -> result;
//
// Therefore, we define a subtrait that is automatically implemented for closures.

/// Trait for managed source builders.
///
/// # Example
/// ```
/// use std::time::Duration;
/// use alumet::pipeline::elements::source::builder::{ManagedSource, ManagedSourceBuilder, ManagedSourceBuildContext};
/// use alumet::pipeline::{trigger, Source};
///
/// fn build_my_source() -> anyhow::Result<Box<dyn Source>> {
///     todo!("build a new source")
/// }
///
/// let builder: &dyn ManagedSourceBuilder = &|ctx: &mut dyn ManagedSourceBuildContext| {
///     let source = build_my_source()?;
///     Ok(ManagedSource {
///         trigger_spec: trigger::TriggerSpec::at_interval(Duration::from_secs(1)),
///         source,
///     })
/// };
/// ```
pub trait ManagedSourceBuilder: FnOnce(&mut dyn ManagedSourceBuildContext) -> anyhow::Result<ManagedSource> {}
impl<F> ManagedSourceBuilder for F where F: FnOnce(&mut dyn ManagedSourceBuildContext) -> anyhow::Result<ManagedSource> {}

/// Trait for autonomous source builders.
///
/// # Example
/// ```
/// use alumet::pipeline::elements::source::builder::{AutonomousSourceBuilder, AutonomousSourceBuildContext};
/// use alumet::pipeline::{trigger, Source};
/// use alumet::measurement::MeasurementBuffer;
///
/// use std::time::Duration;
/// use tokio::sync::mpsc::Sender;
/// use tokio_util::sync::CancellationToken;
///
/// async fn my_autonomous_source(shutdown: CancellationToken, tx: Sender<MeasurementBuffer>) -> anyhow::Result<()> {
///     let fut = async { todo!("async trigger") };
///     loop {
///         tokio::select! {
///             _ = shutdown.cancelled() => {
///                 // stop here
///                 break;
///             },
///             _ = fut => {
///                 todo!("measure something and send it to tx");
///             }
///         }
///     }
///     Ok(())
/// }
///
/// let builder: &dyn AutonomousSourceBuilder = &|ctx: &mut dyn AutonomousSourceBuildContext, shutdown: CancellationToken, tx: Sender<MeasurementBuffer>| {
///     let source = Box::pin(my_autonomous_source(shutdown, tx));
///     Ok(source)
///     // No trigger here, the source is autonomous and triggers itself.
/// };
/// ```
pub trait AutonomousSourceBuilder:
    FnOnce(
    &mut dyn AutonomousSourceBuildContext,
    CancellationToken,
    Sender<MeasurementBuffer>,
) -> anyhow::Result<AutonomousSource>
{
}
impl<F> AutonomousSourceBuilder for F where
    F: FnOnce(
        &mut dyn AutonomousSourceBuildContext,
        CancellationToken,
        Sender<MeasurementBuffer>,
    ) -> anyhow::Result<AutonomousSource>
{
}

/// A source builder, for a managed or autonomous source.
///
/// Use this type in the pipeline Builder.
pub enum SourceBuilder {
    Managed(Box<dyn ManagedSourceBuilder>),
    Autonomous(Box<dyn AutonomousSourceBuilder>),
}

/// Like [`SourceBuilder`] but with a [`Send`] bound on the builder.
///
/// Use this type in the pipeline control loop.
pub enum SendSourceBuilder {
    Managed(Box<dyn ManagedSourceBuilder + Send>),
    Autonomous(Box<dyn AutonomousSourceBuilder + Send>),
}

impl From<SendSourceBuilder> for SourceBuilder {
    fn from(value: SendSourceBuilder) -> Self {
        match value {
            SendSourceBuilder::Managed(b) => SourceBuilder::Managed(b),
            SendSourceBuilder::Autonomous(b) => SourceBuilder::Autonomous(b),
        }
    }
}

impl std::fmt::Debug for SourceBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Managed(_) => f.debug_tuple("Managed").field(&"Box<dyn _>").finish(),
            Self::Autonomous(_) => f.debug_tuple("Autonomous").field(&"Box<dyn _>").finish(),
        }
    }
}

impl std::fmt::Debug for SendSourceBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Managed(_) => f.debug_tuple("Managed").field(&"Box<dyn _>").finish(),
            Self::Autonomous(_) => f.debug_tuple("Autonomous").field(&"Box<dyn _>").finish(),
        }
    }
}

/// Information required to register a new managed source to the measurement pipeline.
pub struct ManagedSource {
    pub trigger_spec: TriggerSpec,
    pub source: Box<dyn Source>,
}

pub(super) struct BuildContext<'a> {
    pub(super) metrics: &'a MetricRegistry,
    pub(super) metrics_r: &'a MetricReader,
    pub(super) metrics_tx: &'a MetricSender,
}

/// Context accessible when building a managed source.
pub trait ManagedSourceBuildContext {
    /// Retrieves a metric by its name.
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
}

/// Context accessible when building an autonomous source (not triggered by Alumet).
pub trait AutonomousSourceBuildContext {
    /// Retrieves a metric by its name.
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)>;
    /// Returns a `MetricReader`, which allows to access the metric registry.
    fn metrics_reader(&self) -> MetricReader;
    /// Returns a `MetricSender`, which allows to register new metrics while the pipeline is running.
    fn metrics_sender(&self) -> MetricSender;
}

impl ManagedSourceBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        self.metrics.by_name(name)
    }
}

impl AutonomousSourceBuildContext for BuildContext<'_> {
    fn metric_by_name(&self, name: &str) -> Option<(RawMetricId, &Metric)> {
        ManagedSourceBuildContext::metric_by_name(self, name)
    }

    fn metrics_reader(&self) -> MetricReader {
        self.metrics_r.clone()
    }

    fn metrics_sender(&self) -> MetricSender {
        self.metrics_tx.clone()
    }
}
