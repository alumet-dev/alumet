use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::AttributeValue,
    pipeline::elements::source::trigger::TriggerSpec,
    plugin::{AlumetPluginStart, ConfigTable, PluginMetadata, rust::AlumetPlugin},
    resources::ResourceConsumer,
    test::RuntimeExpectations,
};
use util_cgroups::{Cgroup, CgroupHierarchy, CgroupVersion};
use util_cgroups_plugins::{
    metrics::{AugmentedMetrics, Metrics},
    v1::CgroupV1Probe,
};

const TIMEOUT: Duration = Duration::from_secs(2);
const SOURCE: &str = "cgroupV1Probe";
const CGROUP_NAME: &str = "my.platypus";
// common attribute added to every point of the probe
const COMMON_ATTR_KEY: &str = "job_id";
const COMMON_ATTR_VALUE: u64 = 42;

struct TestPlugin {
    cgroup_dir: PathBuf,
}

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "v1-probe-test"
    }

    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let cgroup_dir = config.0["cgroup_dir"].as_str().unwrap().into();
        Ok(Box::new(Self { cgroup_dir }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // in cgroup v1, the probe reads the file of each controller of the hierarchy
        let hierarchy = CgroupHierarchy::manually_unchecked(
            self.cgroup_dir.parent().unwrap(),
            CgroupVersion::V1,
            vec!["cpuacct", "memory"],
        );

        let cgroup = Cgroup::from_fs_path(&hierarchy, self.cgroup_dir.clone());

        let metrics = Metrics::create(alumet)?;
        let common_attrs = [(COMMON_ATTR_KEY.to_owned(), AttributeValue::U64(COMMON_ATTR_VALUE))];
        let augmented_metrics = AugmentedMetrics::with_common_attr_slice(&metrics, &common_attrs);

        let consumer = ResourceConsumer::ControlGroup {
            path: cgroup.canonical_path().to_owned().into(),
        };

        let probe = CgroupV1Probe::new(cgroup, consumer, augmented_metrics)?;
        alumet.add_source(
            SOURCE,
            Box::new(probe),
            TriggerSpec::at_interval(Duration::from_secs(1)),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn agent_starts(cgroup_dir: &Path) -> bool {
    let plugins = plugin_set(cgroup_dir);
    match agent::Builder::new(plugins).build_and_start() {
        Ok(agent) => {
            agent.pipeline.control_handle().shutdown();
            agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
            true
        }
        Err(_) => false,
    }
}

fn run_probe(cgroup_dir: &Path, runtime: RuntimeExpectations) {
    let plugins = plugin_set(cgroup_dir);
    let agent = agent::Builder::new(plugins)
        .with_expectations(runtime)
        .build_and_start()
        .expect("agent should start");
    agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
}

// Helper function which return a plugin set from a cgroup folder path
fn plugin_set(cgroup_dir: &Path) -> PluginSet {
    let mut config = toml::Table::new();
    config.insert("cgroup_dir".into(), cgroup_dir.to_str().unwrap().into());
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<TestPlugin>(),
        enabled: true,
        config: Some(config),
    });
    plugins
}

/// Creates a cgroup directory in a temporary root, with the files of the `cpuacct` and `memory` controllers.
fn cgroup_dir() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(CGROUP_NAME);
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("cpuacct.usage"), "0\n").unwrap();
    std::fs::write(dir.join("memory.usage_in_bytes"), "0\n").unwrap();
    (root, dir)
}

/// The probe can be created on a cgroup that has the files of its controllers.
#[test]
fn full_cgroup() {
    let (_root, dir) = cgroup_dir();
    assert!(agent_starts(&dir));
}

/// Unlike cgroup v2, a missing file is an error: the controller exists, so its file should exist too.
#[test]
fn missing_file() {
    let (_root, dir) = cgroup_dir();
    std::fs::remove_file(dir.join("memory.usage_in_bytes")).unwrap();
    // Should not start
    assert!(!agent_starts(&dir));
}

mod poll {
    use super::*;
    use alumet::{
        measurement::{MeasurementBuffer, MeasurementPoint},
        metrics::registry::MetricRegistry,
        pipeline::naming::SourceName,
    };

    // Name of the probe source
    fn source() -> SourceName {
        SourceName::from_str(TestPlugin::name(), SOURCE)
    }

    /// The points of the metric named `metric`.
    fn points<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer, metric: &str) -> Vec<&'a MeasurementPoint> {
        buf.iter()
            .filter(|p| metrics.by_id(&p.metric).unwrap().name == metric)
            .collect()
    }

    /// The value of the attribute `key`, as a string.
    fn attr(point: &MeasurementPoint, key: &str) -> Option<String> {
        point.attributes().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    /// The memory used by the cgroup is measured in bytes.
    #[test]
    fn memory() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("memory.usage_in_bytes"), "5555\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let memory_usage_points = points(ctx.metrics(), ctx.measurements(), "memory_usage");
                assert_eq!(memory_usage_points.len(), 1);
                assert_eq!(memory_usage_points[0].value.as_u64(), 5555);
            },
        );
        run_probe(&dir, runtime);
    }

    /// The cpu time is a difference between two polls: nothing is measured on the first one.
    #[test]
    fn cpu_first_poll() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("cpuacct.usage"), "7474\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let cpu_time_delta_points = points(ctx.metrics(), ctx.measurements(), "cpu_time_delta");
                let cpu_percent_points = points(ctx.metrics(), ctx.measurements(), "cpu_percent");
                assert_eq!(cpu_time_delta_points.len(), 0);
                assert_eq!(cpu_percent_points.len(), 0);
            },
        );
        run_probe(&dir, runtime);
    }

    /// The cpu time consumed between two polls is measured in ns (cpuacct.usage is already in ns),
    /// with its percentage. Unlike cgroup v2, there is only the "total" kind.
    #[test]
    fn cpu_time_delta() {
        let (_root, dir) = cgroup_dir();
        let cpuacct_usage = dir.join("cpuacct.usage");
        std::fs::write(&cpuacct_usage, "1000\n").unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(source(), || (), |_| ())
            .test_source(
                source(),
                move || std::fs::write(&cpuacct_usage, "4000\n").unwrap(),
                |ctx| {
                    let cpu_time_delta_points = points(ctx.metrics(), ctx.measurements(), "cpu_time_delta");
                    let cpu_percent_points = points(ctx.metrics(), ctx.measurements(), "cpu_percent");
                    assert_eq!(cpu_time_delta_points.len(), 1);
                    assert_eq!(cpu_time_delta_points[0].value.as_u64(), 4000 - 1000);
                    assert_eq!(attr(cpu_time_delta_points[0], "kind").as_deref(), Some("total"));
                    assert_eq!(cpu_percent_points.len(), 1);
                    assert!(cpu_percent_points[0].value.as_f64() > 0.0);
                    assert_eq!(attr(cpu_percent_points[0], "kind").as_deref(), Some("total"));
                },
            );
        run_probe(&dir, runtime);
    }

    /// Every point is measured for the cgroup, and has the common attributes of the probe.
    #[test]
    fn consumer_and_common_attrs() {
        let (_root, dir) = cgroup_dir();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let buf = ctx.measurements();
                assert!(!buf.is_empty());
                let consumer = ResourceConsumer::ControlGroup {
                    path: format!("/{CGROUP_NAME}").into(),
                };
                for point in buf.iter() {
                    assert_eq!(point.consumer, consumer);
                    assert_eq!(attr(point, COMMON_ATTR_KEY), Some(COMMON_ATTR_VALUE.to_string()));
                }
            },
        );
        run_probe(&dir, runtime);
    }
}
