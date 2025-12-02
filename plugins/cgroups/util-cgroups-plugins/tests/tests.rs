use std::{
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{
        AttributeValue, MeasurementAccumulator, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue,
    },
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
use anyhow::Context;
use expecting::expect_err;
use util_cgroups::{CgroupHierarchy, CgroupVersion, hierarchy::find_user_app_slice};

use serde::{Deserialize, Serialize};
use util_cgroups_plugins::{
    cgroup_events::CgroupFsMountCallback,
    job_annotation_transform::{
        CachedCgroupHierarchy, JobAnnotationTransform, JobTagger, OptionalSharedHierarchy, SharedCgroupHierarchy,
    },
};

const SYSFS_CGROUP: &str = "/sys/fs/cgroup";

#[derive(Clone)]
struct Tagger;

impl JobTagger for Tagger {
    fn attributes_for_cgroup(
        &mut self,
        _cgroup: &util_cgroups::Cgroup,
    ) -> Vec<(String, alumet::measurement::AttributeValue)> {
        return vec![
            ("user_id".to_string(), AttributeValue::U64(1000)),
            ("job_id".to_string(), AttributeValue::String("123456".to_string())),
        ];
    }
}

// This structure is used as a dumb plugin to initialise a cgroupv2 hierarchy and the
// transform plugin we want to test.
struct DumbOARPlugin;
// This structure is used as a dumb plugin without initializing a cgroupv2 hierarchy and the
// transform plugin we want to test.
struct DumbOARPlugin2;

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

        shared.set(CgroupHierarchy::manually_unchecked(
            "/",
            CgroupVersion::V2,
            vec!["cpuset"],
        ));

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

impl AlumetPlugin for DumbOARPlugin2 {
    fn name() -> &'static str {
        "OARRR2"
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

#[test]
fn test_correct_transform() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }
    // let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();

    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP))?;

    // Create cgroupv2 hierarchy
    let cgroup_dir_parent =
        tempfile::tempdir_in(&app_slice).with_context(|| format!("failed to create cgroup in {app_slice:?}"))?;
    let cgroup_dir_job = cgroup_dir_parent
        .path()
        .join(format!("oar.slice/oar-u1000.scope/oar-u1000-j123456"));
    std::fs::create_dir_all(&cgroup_dir_job)?;

    log::info!("cgroup created at {:?}", cgroup_dir_job);

    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbOARPlugin>(),
        enabled: true,
        config: None,
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        prepare_mock_measurements(ctx, cgroup_dir_job.clone()).expect("failed to prepare mock points")
    };

    // With this closure, we want to check that the measurement contains the correct
    // number of element in the measurement buffer and then we want to check if all
    // measurements with "cgroups" as ressource consumer have a "job_id" attribute
    let check_output = move |ctx: &mut TransformCheckOutputContext| {
        let measurements = ctx.measurements();
        assert_eq!(3, measurements.len());
        for measure in measurements {
            if let ResourceConsumer::ControlGroup { .. } = measure.consumer {
                assert!(measure.attributes_keys().any(|attr| attr == "job_id"));
            };
        }
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

#[test]
fn test_cgroups_files_not_created() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }
    // let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();

    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP)).unwrap();

    // Create cgroupv2 hierarchy
    let cgroup_dir_parent = tempfile::tempdir_in(&app_slice)
        .with_context(|| format!("failed to create cgroup in {app_slice:?}"))
        .unwrap();
    let cgroup_dir_job = cgroup_dir_parent
        .path()
        .join(format!("oar.slice/oar-u1000.scope/oar-u1000-j123456"));

    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbOARPlugin>(),
        enabled: true,
        config: None,
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        prepare_mock_measurements(ctx, cgroup_dir_job.clone()).expect("failed to prepare mock points")
    };

    // With this closure, we want to check that the measurement contains the correct
    // number of element in the measurement buffer and then we want to check if all
    // measurements with "cgroups" as ressource consumer have a "job_id" attribute
    let check_output = move |ctx: &mut TransformCheckOutputContext| {
        let measurements = ctx.measurements();
        assert_eq!(3, measurements.len());
        for measure in measurements {
            if let ResourceConsumer::ControlGroup { .. } = measure.consumer {
                assert!(measure.attributes_keys().any(|attr| attr == "job_id"));
            };
        }
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

    expect_err!(agent.wait_for_shutdown(Duration::from_secs(2)));
    Ok(())
}

#[test]
fn test_cgroup_v2_hierarchy_not_created() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }
    // let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP)).unwrap();

    // Create cgroupv2 hierarchy
    let cgroup_dir_parent = tempfile::tempdir_in(&app_slice)
        .with_context(|| format!("failed to create cgroup in {app_slice:?}"))
        .unwrap();
    let cgroup_dir_job = cgroup_dir_parent
        .path()
        .join(format!("oar.slice/oar-u1000.scope/oar-u1000-j123456"));

    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbOARPlugin2>(),
        enabled: true,
        config: None,
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        prepare_mock_measurements(ctx, cgroup_dir_job.clone()).expect("failed to prepare mock points")
    };

    // With this closure, we want to check that the measurement contains the correct
    // number of element in the measurement buffer and then we want to check if all
    // measurements with "cgroups" as ressource consumer have a "job_id" attribute
    let check_output = move |ctx: &mut TransformCheckOutputContext| {
        let measurements = ctx.measurements();
        for measure in measurements {
            if let ResourceConsumer::ControlGroup { .. } = measure.consumer {
                assert!(measure.attributes_keys().any(|attr| attr == "job_id"));
            };
        }
    };

    let runtime_expectations = RuntimeExpectations::new().test_transform(
        TransformName::from_str("OARRR2", "oar-annotation"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    expect_err!(agent.wait_for_shutdown(Duration::from_secs(2)));
    Ok(())
}

#[test]
fn test_no_cgroupv2_at_all() -> anyhow::Result<()> {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return Ok(());
    }
    // let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).try_init();
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<DumbOARPlugin2>(),
        enabled: true,
        config: None,
    });

    let make_input = move |ctx: &mut TransformCheckInputContext| -> MeasurementBuffer {
        prepare_mock_measurements(ctx, PathBuf::from("/toto/sys/")).expect("failed to prepare mock points")
    };

    // With this closure, we want to check that the measurement contains the correct
    // number of element in the measurement buffer and then we want to check if all
    // measurements with "cgroups" as ressource consumer have a "job_id" attribute
    let check_output = move |ctx: &mut TransformCheckOutputContext| {
        let measurements = ctx.measurements();
        assert_eq!(3, measurements.len());
        for measure in measurements {
            if let ResourceConsumer::ControlGroup { .. } = measure.consumer {
                assert!(measure.attributes_keys().any(|attr| attr == "job_id"));
            };
        }
    };

    let runtime_expectations = RuntimeExpectations::new().test_transform(
        TransformName::from_str("OARRR2", "oar-annotation"),
        make_input,
        check_output,
    );

    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime_expectations)
        .build_and_start()
        .unwrap();

    expect_err!(agent.wait_for_shutdown(Duration::from_secs(2)));

    Ok(())
}

#[test]
fn test_on_cgroupfs_mounted() -> () {
    if std::env::var_os("SKIP_CGROUPFS_TESTS").is_some() {
        println!("skipped because SKIP_CGROUPFS_TESTS is set");
        return ();
    }
    let app_slice = find_user_app_slice(Path::new(SYSFS_CGROUP)).unwrap();
    // Create cgroupv2 hierarchy
    let cgroup_dir_parent = tempfile::tempdir_in(&app_slice)
        .with_context(|| format!("failed to create cgroup in {app_slice:?}"))
        .unwrap();
    let cgroup_dir_job = cgroup_dir_parent
        .path()
        .join(format!("oar.slice/oar-u1000.scope/oar-u1000-j123456"));
    std::fs::create_dir_all(&cgroup_dir_job).unwrap();

    let mut shared_hierarchy = OptionalSharedHierarchy::default();
    let cgroup_hierarchy = vec![CgroupHierarchy::manually_unchecked(
        cgroup_dir_job,
        CgroupVersion::V2,
        vec!["cpuset"],
    )];
    let ret = shared_hierarchy.on_cgroupfs_mounted(&cgroup_hierarchy);
    assert!(ret.is_ok());

    ()
}

fn prepare_mock_measurements(ctx: &mut TransformCheckInputContext, path: PathBuf) -> anyhow::Result<MeasurementBuffer> {
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
    let t: Timestamp = Timestamp::from(UNIX_EPOCH);
    let t2: Timestamp = Timestamp::from(UNIX_EPOCH + Duration::from_secs(1));
    m.push(create_point(
        t,
        metric_a,
        ResourceConsumer::ControlGroup {
            path: path.to_str().unwrap().to_owned().into(),
        },
        10,
    ));
    m.push(create_point(
        t2,
        metric_a,
        ResourceConsumer::ControlGroup {
            path: path.to_str().unwrap().to_owned().into(),
        },
        11,
    ));
    m.push(create_point(t, metric_a, ResourceConsumer::LocalMachine, 10));

    Ok(m)
}
