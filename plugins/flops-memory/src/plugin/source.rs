use std::{collections::HashMap, fs::File, sync::Arc};

use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp},
    metrics::TypedMetricId,
    pipeline::{Source, elements::error::PollError},
    resources::{Resource, ResourceConsumer},
};
use anyhow::{Context, bail};
use plugin_perf::{
    event::ParsedEvent,
    group::{EventGroup, EventGroupBuilder, Target},
};

use crate::formula;

pub(super) use plugin_perf::group::Accuracy;

use super::Runtime;

pub(super) struct RuntimeMetric {
    pub(super) metric: TypedMetricId<f64>,
    pub(super) name: String,
    pub(super) formula: String,
    pub(super) inputs: Vec<(String, usize)>,
}

struct GroupReadout {
    counters: Vec<EventGroup>,
    resource: Resource,
    consumer: ResourceConsumer,
}

pub(super) struct FlopsSource {
    groups: Vec<GroupReadout>,
    counter_count: usize,
    metrics: Vec<RuntimeMetric>,
}

impl FlopsSource {
    pub(super) fn for_target(
        runtime: &Runtime,
        target: Target,
        resource: Resource,
        consumer: ResourceConsumer,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            groups: vec![GroupReadout {
                counters: open_counters(&runtime.counters, target)?,
                resource,
                consumer,
            }],
            counter_count: runtime.counters.len(),
            metrics: clone_runtime_metrics(&runtime.metrics),
        })
    }

    pub(super) fn for_cgroup(runtime: &Runtime, path: String, fd: Arc<File>, cpus: Vec<u32>) -> anyhow::Result<Self> {
        let mut groups = Vec::with_capacity(cpus.len());
        for cpu in cpus {
            let target = Target::Cgroup { fd: fd.clone(), cpu };
            groups.push(GroupReadout {
                counters: open_counters(&runtime.counters, target)
                    .with_context(|| format!("opening FLOPS counters for cgroup {path} on CPU {cpu}"))?,
                resource: Resource::CpuCore { id: cpu },
                consumer: ResourceConsumer::ControlGroup {
                    path: path.clone().into(),
                },
            });
        }
        if groups.is_empty() {
            bail!("no online CPUs are available for cgroup {path}");
        }
        Ok(Self {
            groups,
            counter_count: runtime.counters.len(),
            metrics: clone_runtime_metrics(&runtime.metrics),
        })
    }
}

impl Source for FlopsSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        for readout in &mut self.groups {
            let values = readout
                .counters
                .iter_mut()
                .map(|group| {
                    group.read()?;
                    let [value] = group.values() else {
                        bail!("expected exactly one counter per FLOPS perf group");
                    };
                    Ok((*value, group.accuracy()))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            if values.len() != self.counter_count {
                return Err(anyhow::anyhow!(
                    "perf returned {} counters but {} were configured",
                    values.len(),
                    self.counter_count
                )
                .into());
            }
            let (values, accuracies): (Vec<_>, Vec<_>) = values.into_iter().unzip();
            for metric in &self.metrics {
                let value = evaluate_metric(&metric.formula, &metric.inputs, &values)
                    .with_context(|| format!("evaluating LIKWID formula for '{}'", metric.name))?;
                let accuracy = metric_accuracy(&metric.inputs, &accuracies)?;
                let point = MeasurementPoint::new(
                    timestamp,
                    metric.metric,
                    readout.resource.clone(),
                    readout.consumer.clone(),
                    value,
                )
                .with_attr("accuracy", accuracy.as_str());
                measurements.push(point);
            }
        }
        Ok(())
    }
}

fn open_counters(events: &[ParsedEvent], target: Target) -> anyhow::Result<Vec<EventGroup>> {
    // A group must fit on the PMU as a whole. Separate groups let the kernel multiplex
    // arbitrarily many FLOPS events instead of starving one oversized group.
    events
        .iter()
        .map(|event| {
            let mut builder = EventGroupBuilder::open(target.clone(), true)
                .with_context(|| format!("opening FLOPS counter {}", event.name()))?;
            builder.add(event)?;
            builder
                .build()
                .with_context(|| format!("enabling FLOPS counter {}", event.name()))
        })
        .collect()
}

pub(super) fn evaluate_metric(formula: &str, inputs: &[(String, usize)], values: &[u64]) -> anyhow::Result<f64> {
    let variables = inputs
        .iter()
        .map(|(name, index)| {
            let value = values
                .get(*index)
                .with_context(|| format!("missing raw counter value for '{name}' at index {index}"))?;
            Ok((name.as_str(), *value as f64))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;
    formula::evaluate(formula, &variables)
}

pub(super) fn metric_accuracy(inputs: &[(String, usize)], accuracies: &[Accuracy]) -> anyhow::Result<Accuracy> {
    inputs.iter().try_fold(Accuracy::Exact, |result, (name, index)| {
        let accuracy = accuracies
            .get(*index)
            .with_context(|| format!("missing accuracy for raw counter '{name}' at index {index}"))?;
        Ok(result.max(*accuracy))
    })
}

fn clone_runtime_metrics(metrics: &[RuntimeMetric]) -> Vec<RuntimeMetric> {
    metrics
        .iter()
        .map(|metric| RuntimeMetric {
            metric: metric.metric,
            name: metric.name.clone(),
            formula: metric.formula.clone(),
            inputs: metric.inputs.clone(),
        })
        .collect()
}
