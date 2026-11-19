mod config;
mod source;

use std::{collections::HashMap, fs::File, io, os::raw::c_int, sync::Arc, time::Duration};

use alumet::{
    pipeline::{
        control::{matching::SourceMatcher, request},
        elements::source::trigger::TriggerSpec,
        matching::{SourceNamePattern, StringPattern},
    },
    plugin::{
        AlumetPostStart, ConfigTable, event as alumet_event_bus,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    resources::{Resource, ResourceConsumer},
    units::Unit,
};
use anyhow::{Context, bail};
use plugin_perf::{
    event::{self, ParsedEvent, Scope},
    group::Target,
};

use crate::cpuinfo::{CpuInfo, read_cpu_info};
use config::{ArchitectureRecord, Config, MetricDefinition, MetricsDocument};
use source::{FlopsSource, RuntimeMetric};

#[cfg(test)]
use source::{Accuracy, evaluate_metric, metric_accuracy};

const METRICS_JSON: &str = include_str!("../../data/likwid_metrics.json");

const RLIMIT_NOFILE: c_int = 7;

#[repr(C)]
struct FileDescriptorLimit {
    current: usize,
    maximum: usize,
}

unsafe extern "C" {
    fn getrlimit(resource: c_int, limits: *mut FileDescriptorLimit) -> c_int;
    fn setrlimit(resource: c_int, limits: *const FileDescriptorLimit) -> c_int;
}

pub struct FlopsMemoryPlugin {
    config: Config,
    runtime: Option<Arc<Runtime>>,
}

struct Runtime {
    poll_interval: Duration,
    flush_interval: Duration,
    add_source_in_pause_state: bool,
    counters: Vec<ParsedEvent>,
    metrics: Vec<RuntimeMetric>,
}

struct CounterPlan {
    counters: Vec<ParsedEvent>,
    metrics: Vec<PlannedMetric>,
}

struct PlannedMetric {
    definition: MetricDefinition,
    inputs: Vec<(String, usize)>,
}

impl CounterPlan {
    fn new(definitions: &[MetricDefinition]) -> anyhow::Result<Self> {
        let mut counters = Vec::new();
        let mut counter_indices = HashMap::new();
        let mut metrics = Vec::with_capacity(definitions.len());
        for definition in definitions {
            if !definition.unresolved_dependencies.is_empty() {
                bail!(
                    "LIKWID metric '{}' has unresolved event dependencies: {}",
                    definition.name,
                    definition.unresolved_dependencies.join(", ")
                );
            }
            if definition.events.is_empty() {
                bail!("LIKWID metric '{}' has no event counters", definition.name);
            }

            let mut inputs = Vec::with_capacity(definition.events.len());
            for (counter, event_spec) in &definition.events {
                if event_spec == "not found" {
                    bail!(
                        "LIKWID metric '{}' uses unresolved counter '{counter}'",
                        definition.name
                    );
                }
                let counter_index = if let Some(&index) = counter_indices.get(event_spec) {
                    index
                } else {
                    let mut parsed = event::parse(event_spec).with_context(|| {
                        format!("could not parse raw event '{event_spec}' for '{}'", definition.name)
                    })?;
                    if parsed.len() != 1 {
                        bail!("event '{event_spec}' did not resolve to exactly one perf event");
                    }
                    let parsed = parsed.pop().expect("length checked");
                    if !matches!(parsed.scope(), Scope::TaskAttached { .. }) {
                        bail!("LIKWID event '{event_spec}' must be task-attached to support cgroups");
                    }
                    let index = counters.len();
                    counters.push(parsed);
                    counter_indices.insert(event_spec.clone(), index);
                    index
                };
                inputs.push((counter.clone(), counter_index));
            }
            metrics.push(PlannedMetric {
                definition: definition.clone(),
                inputs,
            });
        }
        Ok(Self { counters, metrics })
    }
}

impl AlumetPlugin for FlopsMemoryPlugin {
    fn name() -> &'static str {
        "flops-memory"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self {
            config: deserialize_config(config)?,
            runtime: None,
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let cpu = read_cpu_info().context("could not identify the CPU architecture")?;
        let document = read_metrics_document(METRICS_JSON)?;
        let Some(record) = select_architecture(&document, &cpu) else {
            log::info!(
                "CPU family {}, model {} (stepping {}) is not supported by the embedded LIKWID metrics; \
                 flops-memory will not start a source",
                cpu.family,
                cpu.model,
                cpu.stepping.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            );
            self.runtime = None;
            return Ok(());
        };

        let definitions = &record.metrics.flops;
        if definitions.is_empty() {
            log::warn!("No FLOPS metrics are defined for this CPU architecture.");
            self.runtime = None;
            return Ok(());
        }

        let plan = CounterPlan::new(definitions)?;
        increase_file_descriptors_soft_limit().context("could not increase file descriptor limit")?;
        log::info!(
            "Configuring all {} FLOPS metrics using {} distinct raw events",
            plan.metrics.len(),
            plan.counters.len()
        );
        for (index, counter) in plan.counters.iter().enumerate() {
            log::debug!("FLOPS raw counter {index}: {}", counter.name());
        }
        let mut metrics = Vec::with_capacity(plan.metrics.len());
        for PlannedMetric { definition, inputs } in plan.metrics {
            let unit = unit_from_likwid(&definition.units)?;
            let metric =
                alumet.create_metric::<f64>(&definition.name, unit, format!("LIKWID metric {}", definition.name))?;
            metrics.push(RuntimeMetric {
                metric,
                name: definition.name,
                formula: definition.formula,
                inputs,
            });
        }

        self.runtime = Some(Arc::new(Runtime {
            poll_interval: self.config.poll_interval,
            flush_interval: self.config.flush_interval,
            add_source_in_pause_state: self.config.add_source_in_pause_state,
            counters: plan.counters,
            metrics,
        }));
        Ok(())
    }

    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let Some(runtime) = self.runtime.clone() else {
            return Ok(());
        };

        let pipeline_control_start = alumet.pipeline_control();
        let pipeline_control_end = alumet.pipeline_control();
        let async_start = alumet.async_runtime().clone();
        let async_end = alumet.async_runtime().clone();

        alumet_event_bus::start_consumer_measurement().subscribe(move |event| {
            for consumer in event.0 {
                let (target, resource, consumer, source_name) = match consumer {
                    ResourceConsumer::Process { pid } => (
                        Target::Process { pid },
                        Resource::LocalMachine,
                        ResourceConsumer::Process { pid },
                        process_source_name(pid),
                    ),
                    ResourceConsumer::ControlGroup { path } => {
                        let absolute_path = format!("/sys/fs/cgroup{path}");
                        let fd = Arc::new(
                            File::open(&absolute_path)
                                .with_context(|| format!("could not open cgroup {absolute_path}"))?,
                        );
                        let cpus = online_cpus().context("could not read online CPUs for cgroup counters")?;
                        let source_name = cgroup_source_name(&path);
                        let source = FlopsSource::for_cgroup(&runtime, path.to_string(), fd, cpus)?;
                        let trigger = TriggerSpec::builder(runtime.poll_interval)
                            .flush_interval(runtime.flush_interval)
                            .build()?;
                        let request = request::create_one().add_source_with_state(
                            &source_name,
                            Box::new(source),
                            trigger,
                            initial_task_state(runtime.add_source_in_pause_state),
                        );
                        async_start.block_on(pipeline_control_start.dispatch(request, Duration::from_secs(1)))?;
                        continue;
                    }
                    _ => continue,
                };

                let source = FlopsSource::for_target(&runtime, target, resource, consumer)?;
                let trigger = TriggerSpec::builder(runtime.poll_interval)
                    .flush_interval(runtime.flush_interval)
                    .build()?;
                let request = request::create_one().add_source_with_state(
                    &source_name,
                    Box::new(source),
                    trigger,
                    initial_task_state(runtime.add_source_in_pause_state),
                );
                async_start.block_on(pipeline_control_start.dispatch(request, Duration::from_secs(1)))?;
            }
            Ok(())
        });

        alumet_event_bus::end_consumer_measurement().subscribe(move |event| {
            for consumer in event.0 {
                let source_name = match consumer {
                    ResourceConsumer::Process { pid } => process_source_name(pid),
                    ResourceConsumer::ControlGroup { path } => cgroup_source_name(path),
                    _ => continue,
                };
                let stop_request = request::source::source(SourceMatcher::Name(SourceNamePattern::new(
                    StringPattern::Exact(FlopsMemoryPlugin::name().to_owned()),
                    StringPattern::Exact(source_name),
                )))
                .stop();
                async_end.block_on(pipeline_control_end.dispatch(stop_request, Duration::from_secs(1)))?;
            }
            Ok(())
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn increase_file_descriptors_soft_limit() -> io::Result<()> {
    let mut limits = FileDescriptorLimit { current: 0, maximum: 0 };
    if unsafe { getrlimit(RLIMIT_NOFILE, &mut limits) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if limits.current < limits.maximum {
        limits.current = limits.maximum;
        if unsafe { setrlimit(RLIMIT_NOFILE, &limits) } != 0 {
            return Err(io::Error::last_os_error());
        }
        log::debug!(
            "Increased file descriptor soft limit to hard limit ({}) for perf counters",
            limits.maximum
        );
    }
    Ok(())
}

fn read_metrics_document(json: &str) -> anyhow::Result<MetricsDocument> {
    serde_json::from_str(json).context("invalid embedded LIKWID metrics JSON")
}

fn select_architecture<'a>(document: &'a MetricsDocument, cpu: &CpuInfo) -> Option<&'a ArchitectureRecord> {
    document
        .architectures
        .get(&cpu.family.to_string())?
        .iter()
        .find(|record| {
            record.definitions.models.iter().any(|model| {
                model.model_code == cpu.model && condition_matches(model.condition.as_deref(), cpu.stepping)
            })
        })
}

fn condition_matches(condition: Option<&str>, stepping: Option<u64>) -> bool {
    let Some(condition) = condition else {
        return true;
    };
    let Some(stepping) = stepping else {
        return false;
    };
    let condition = condition.trim().replace("cpuid_info.stepping", "stepping");
    for operator in ["<=", ">=", "==", "!=", "<", ">"] {
        if let Some((lhs, rhs)) = condition.split_once(operator) {
            let Ok(value) = rhs.trim().parse::<u64>() else {
                return false;
            };
            return lhs.trim() == "stepping"
                && match operator {
                    "<=" => stepping <= value,
                    ">=" => stepping >= value,
                    "==" => stepping == value,
                    "!=" => stepping != value,
                    "<" => stepping < value,
                    ">" => stepping > value,
                    _ => unreachable!(),
                };
        }
    }
    false
}

fn unit_from_likwid(units: &str) -> anyhow::Result<Unit> {
    match units {
        "FLOP" => Ok(Unit::Custom {
            unique_name: "FLOP".to_owned(),
            display_name: "FLOP".to_owned(),
        }),
        "Bytes" => Ok(Unit::Byte),
        other => bail!("unsupported LIKWID metric unit '{other}'"),
    }
}

fn online_cpus() -> anyhow::Result<Vec<u32>> {
    let content = std::fs::read_to_string("/sys/devices/system/cpu/online")
        .context("could not read /sys/devices/system/cpu/online")?;
    let mut cpus = Vec::new();
    for range in content.trim().split(',') {
        let mut bounds = range.splitn(2, '-');
        let start = bounds
            .next()
            .context("empty CPU range")?
            .parse::<u32>()
            .with_context(|| format!("invalid CPU range '{range}'"))?;
        let end = bounds
            .next()
            .map(|end| {
                end.parse::<u32>()
                    .with_context(|| format!("invalid CPU range '{range}'"))
            })
            .transpose()?
            .unwrap_or(start);
        if end < start {
            bail!("invalid CPU range '{range}'");
        }
        cpus.extend(start..=end);
    }
    if cpus.is_empty() {
        bail!("the online CPU list is empty");
    }
    Ok(cpus)
}

fn initial_task_state(paused: bool) -> alumet::pipeline::elements::source::control::TaskState {
    if paused {
        alumet::pipeline::elements::source::control::TaskState::Pause
    } else {
        alumet::pipeline::elements::source::control::TaskState::Run
    }
}

fn process_source_name(pid: u32) -> String {
    format!("source-pid[{pid}]")
}

fn cgroup_source_name(path: impl std::fmt::Display) -> String {
    format!("source-cgroup[{path}]")
}

#[cfg(test)]
#[path = "../../tests/lib.rs"]
mod tests;
