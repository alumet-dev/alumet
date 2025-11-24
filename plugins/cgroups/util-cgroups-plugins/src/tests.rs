use std::{borrow::Cow, time::{Duration, UNIX_EPOCH}};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::{
        Source,
        elements::{error::PollError, source::trigger::TriggerSpec},
        naming::TransformName,
    },
    plugin::{PluginMetadata, rust::AlumetPlugin},
    resources::{Resource, ResourceConsumer},
    test::{
        RuntimeExpectations,
        runtime::{TransformCheckInputContext, TransformCheckOutputContext},
    },
    units::Unit,
};

use crate::{job_annotation_transform::{
    CachedCgroupHierarchy, JobAnnotationTransform, JobTagger, OptionalSharedHierarchy, SharedCgroupHierarchy,
}, metrics};
use serde::{Deserialize, Serialize};

use lazy_static::lazy_static;

lazy_static! {
    static ref T: Timestamp = Timestamp::from(UNIX_EPOCH);
    static ref T2: Timestamp = Timestamp::from(UNIX_EPOCH + Duration::from_secs(1));
}

#[derive(Clone)]
struct Tagger;

impl JobTagger for Tagger {
    fn attributes_for_cgroup(
        &mut self,
        _cgroup: &util_cgroups::Cgroup,
    ) -> Vec<(String, alumet::measurement::AttributeValue)> {
        return vec![];
    }
}

struct DumbOARPlugin;

impl AlumetPlugin for DumbOARPlugin {
    fn name() -> &'static str {
        "OARRR"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(None)
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let metric_a = alumet.create_metric("metric_a", Unit::Unity, "Some metric for tests purpose")?;
        let tagger = Tagger {};
        let mut shared_hierarchy = OptionalSharedHierarchy::default();

        let source = Box::new(DumbOARSource { _metric_a: metric_a });
        alumet.add_source("tests", source, TriggerSpec::at_interval(Duration::from_secs(1)))?;

        let shared = SharedCgroupHierarchy::default();
        shared_hierarchy.enable(shared.clone());

        let transform = JobAnnotationTransform {
            tagger: tagger.clone(),
            cgroup_v2_hierarchy: CachedCgroupHierarchy::new(shared),
        };
        alumet.add_transform("oar-annotation", Box::new(transform))?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct DumbOARSource {
    _metric_a: TypedMetricId<u64>,
}

impl Source for DumbOARSource {
    fn poll(&mut self, _measurements: &mut MeasurementAccumulator, _timestamp: Timestamp) -> Result<(), PollError> {
        Ok(())
    }
}
fn string_vec(values: &[&str]) -> Vec<String> {
    values.into_iter().map(|s| s.to_string()).collect()
}

#[test]
fn run_test_with_config() -> anyhow::Result<()> {
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbOARPlugin>(),
        enabled: true,
        config: None,
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        for metric in ctx.metrics().iter(){ 
            println!("----ctx is: {:?}", metric);
        }
        prepare_mock_measurements(ctx).expect("failed to prepare mock points")
    };

    let check_output = move |ctx: &mut TransformCheckOutputContext| {
        let measurements = ctx.measurements();
        // assert_measurement_counts(measurements, expected_counts.clone());
        for measure in measurements {
            println!("|||||||| {:?}", measure)
        }
        println!("Dans make output");
    };

    let runtime_expectations = RuntimeExpectations::new().test_transform(
        TransformName::from_str("OARRR", "oar-annotation"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(Duration::from_secs(2)).unwrap();

    Ok(())
}

fn prepare_mock_measurements(ctx: &mut TransformCheckInputContext) -> anyhow::Result<MeasurementBuffer> {
    let metric_a = ctx
        .metrics()
        .by_name("metric_a")
        .expect("metric_a metric should exist")
        .0;

    let mut m = MeasurementBuffer::new();

    let create_point = |timestamp, metric, consumer, value| {
        MeasurementPoint::new_untyped(
            timestamp,
            metric,
            Resource::LocalMachine,
            consumer,
            WrappedMeasurementValue::U64(value),
        )
    };

    // metric_a points
    m.push(create_point(*T, metric_a, ResourceConsumer::ControlGroup { path: Cow::Borrowed("/sys/fs/cgroup/toto") } , 10));
    // m.push(create_point(*T2, metric_a, ResourceConsumer::ControlGroup { path: Cow::Borrowed("/sys/fs/cgroup/toto") } , 10));
    // m.push(create_point(*T, metric_a, ResourceConsumer::ControlGroup { path: Cow::Borrowed("/sys/fs/cgroup/toto") }, 10));
    // m.push(create_point(*T, metric_a, ResourceConsumer::LocalMachine, 10));

    Ok(m)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub(crate) oar_version: OarVersion,
    #[serde(with = "humantime_serde")]
    pub(crate) poll_interval: Duration,
    pub(crate) jobs_only: bool,
    /// If `true`, adds attributes like `job_id` to the measurements produced by other plugins.
    /// The default value is `false`.
    ///
    /// The measurements must have the `cgroup` resource consumer, and **cgroup v2** must be used on the node.
    #[serde(default)]
    pub annotate_foreign_measurements: bool,
}

impl Default for Config {
    #[cfg_attr(tarpaulin, ignore)]
    fn default() -> Self {
        Self {
            oar_version: OarVersion::Oar3,
            poll_interval: Duration::from_secs(1),
            jobs_only: true,
            annotate_foreign_measurements: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OarVersion {
    Oar2,
    Oar3,
}
