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
    v2::CgroupV2Probe,
};

const TIMEOUT: Duration = Duration::from_secs(2);
const SOURCE: &str = "cgroupV2Probe";
const CGROUP_NAME: &str = "my.platypus";
// common attribute added to every point of the probe
const COMMON_ATTR_KEY: &str = "job_id";
const COMMON_ATTR_VALUE: u64 = 42;

struct TestPlugin {
    cgroup_dir: PathBuf,
}

impl AlumetPlugin for TestPlugin {
    fn name() -> &'static str {
        "v2-probe-test"
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
        let hierarchy = CgroupHierarchy::manually_unchecked(
            self.cgroup_dir.parent().unwrap(),
            CgroupVersion::V2,
            vec!["cpu", "memory"],
        );

        let cgroup = Cgroup::from_fs_path(&hierarchy, self.cgroup_dir.clone());

        let metrics = Metrics::create(alumet)?;
        let common_attrs = [(COMMON_ATTR_KEY.to_owned(), AttributeValue::U64(COMMON_ATTR_VALUE))];
        let augmented_metrics = AugmentedMetrics::with_common_attr_slice(&metrics, &common_attrs);

        let consumer = ResourceConsumer::ControlGroup {
            path: cgroup.canonical_path().to_owned().into(),
        };

        let probe = CgroupV2Probe::new(cgroup, consumer, augmented_metrics)?;
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

/// Creates an empty cgroup directory in a temporary root
fn cgroup_dir() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(CGROUP_NAME);
    std::fs::create_dir(&dir).unwrap();
    (root, dir)
}

#[test]
fn empty_cgroup() {
    let (_root, dir) = cgroup_dir();
    assert!(agent_starts(&dir));
}

#[test]
fn full_cgroup() {
    let (_root, dir) = cgroup_dir();
    // Write some files
    std::fs::write(dir.join("memory.current"), "5555\n").unwrap();
    std::fs::write(dir.join("memory.max"), "max\n").unwrap();
    std::fs::write(
        dir.join("cpu.stat"),
        "usage_usec 3333\nuser_usec 2222\nsystem_usec 1111\n",
    )
    .unwrap();

    assert!(agent_starts(&dir));
}

#[test]
fn unreadable_file() {
    let (_root, dir) = cgroup_dir();
    // Write a folder instead of file
    std::fs::create_dir(dir.join("memory.stat")).unwrap();
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

    /// The unique point of `metric` with the attribute `key = value`.
    fn point_with_attr<'a>(
        metrics: &MetricRegistry,
        buf: &'a MeasurementBuffer,
        metric: &str,
        key: &str,
        value: &str,
    ) -> &'a MeasurementPoint {
        let points: Vec<_> = points(metrics, buf, metric)
            .into_iter()
            .filter(|p| attr(p, key).as_deref() == Some(value))
            .collect();
        assert_eq!(points.len(), 1, "one point expected for {metric} with {key}={value}");
        points[0]
    }

    /// The value of the unique point of `metric` with the attribute `key = value`.
    fn value_with_attr(metrics: &MetricRegistry, buf: &MeasurementBuffer, metric: &str, key: &str, value: &str) -> u64 {
        point_with_attr(metrics, buf, metric, key, value).value.as_u64()
    }

    /// The value of the unique point of `metric`.
    fn value(metrics: &MetricRegistry, buf: &MeasurementBuffer, metric: &str) -> u64 {
        let points = points(metrics, buf, metric);
        assert_eq!(points.len(), 1, "one point expected for {metric}, got {points:?}");
        points[0].value.as_u64()
    }

    #[test]
    fn memory() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("memory.current"), "5555\n").unwrap();
        std::fs::write(dir.join("memory.max"), "8888\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let memory_usage_points = points(ctx.metrics(), ctx.measurements(), "memory_usage");
                let memory_max_points = points(ctx.metrics(), ctx.measurements(), "memory_max");
                assert_eq!(memory_usage_points.len(), 1);
                assert_eq!(memory_usage_points[0].value.as_u64(), 5555);
                assert_eq!(memory_max_points.len(), 1);
                assert_eq!(memory_max_points[0].value.as_f64(), 8888.0);
            },
        );
        run_probe(&dir, runtime);
    }

    #[test]
    fn cpu_first_poll() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(
            dir.join("cpu.stat"),
            "usage_usec 7474\nuser_usec 2343\nsystem_usec 5131\n",
        )
        .unwrap();

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

    #[test]
    fn cpu_time_delta() {
        let (_root, dir) = cgroup_dir();
        let cpu_stat = dir.join("cpu.stat");
        std::fs::write(&cpu_stat, "usage_usec 0\nuser_usec 0\nsystem_usec 0\n").unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(source(), || (), |_| ())
            .test_source(
                source(),
                move || std::fs::write(&cpu_stat, "usage_usec 3000\nuser_usec 1300\nsystem_usec 1700\n").unwrap(),
                |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    let cpu_time_delta_points = points(ctx.metrics(), ctx.measurements(), "cpu_time_delta");
                    let cpu_percent_points = points(ctx.metrics(), ctx.measurements(), "cpu_percent");
                    assert_eq!(cpu_time_delta_points.len(), 3);
                    assert_eq!(value_with_attr(m, buf, "cpu_time_delta", "kind", "total"), 3_000_000);
                    assert_eq!(value_with_attr(m, buf, "cpu_time_delta", "kind", "user"), 1_300_000);
                    assert_eq!(value_with_attr(m, buf, "cpu_time_delta", "kind", "system"), 1_700_000);
                    assert_eq!(cpu_percent_points.len(), 3);
                },
            );
        run_probe(&dir, runtime);
    }

    /// The cpu usage is a percentage of the time between two polls: as this time (and the number of cores)
    /// is the same for every kind, the percentages are proportional to the cpu time deltas.
    #[test]
    fn cpu_percent() {
        let (_root, dir) = cgroup_dir();
        let cpu_stat = dir.join("cpu.stat");
        std::fs::write(&cpu_stat, "usage_usec 0\nuser_usec 0\nsystem_usec 0\n").unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(source(), || (), |_| ())
            .test_source(
                source(),
                move || std::fs::write(&cpu_stat, "usage_usec 3000\nuser_usec 1300\nsystem_usec 1700\n").unwrap(),
                |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    let percent = |kind| point_with_attr(m, buf, "cpu_percent", "kind", kind).value.as_f64();
                    let (total, user, system) = (percent("total"), percent("user"), percent("system"));
                    assert!(total > 0.0);
                    assert!(
                        (user / system - 1300.0 / 1700.0).abs() < 1e-9,
                        "user / system = {}",
                        user / system
                    );
                    assert!(
                        (total / user - 3000.0 / 1300.0).abs() < 1e-9,
                        "total / user = {}",
                        total / user
                    );
                },
            );
        run_probe(&dir, runtime);
    }

    /// The io pressure is the time (in µs) spent waiting for io between two polls.
    #[test]
    fn io_pressure() {
        let (_root, dir) = cgroup_dir();
        let io_pressure = dir.join("io.pressure");
        std::fs::write(
            &io_pressure,
            "some avg10=0.00 avg60=0.00 avg300=0.00 total=1000\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=500\n",
        )
        .unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(source(), || (), |ctx| {
                // first poll: no previous value, hence no difference
                assert_eq!(points(ctx.metrics(), ctx.measurements(), "io_pressure_some_total").len(), 0);
            })
            .test_source(
                source(),
                move || {
                    std::fs::write(
                        &io_pressure,
                        "some avg10=0.00 avg60=0.00 avg300=0.00 total=4000\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=2500\n",
                    )
                    .unwrap()
                },
                |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    assert_eq!(value(m, buf, "io_pressure_some_total"), 4000 - 1000);
                    assert_eq!(value(m, buf, "io_pressure_full_total"), 2500 - 500);
                },
            );
        run_probe(&dir, runtime);
    }

    /// The fields of memory.stat are measured in bytes, each with its own metric.
    #[test]
    fn memory_stat() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(
            dir.join("memory.stat"),
            "anon 1111\nfile 2222\nkernel_stack 3333\npagetables 4444\nslab_reclaimable 5555\n",
        )
        .unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let (m, buf) = (ctx.metrics(), ctx.measurements());
                assert_eq!(value(m, buf, "cgroup_memory_anonymous"), 1111);
                assert_eq!(value(m, buf, "cgroup_memory_file"), 2222);
                assert_eq!(value(m, buf, "cgroup_memory_kernel_stack"), 3333);
                assert_eq!(value(m, buf, "cgroup_memory_pagetables"), 4444);
                assert_eq!(value(m, buf, "cgroup_slab_reclaimable"), 5555);
            },
        );
        run_probe(&dir, runtime);
    }

    /// The current and maximum swap of the cgroup are measured in bytes.
    #[test]
    fn swap_memory() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("memory.swap.current"), "1234\n").unwrap();
        std::fs::write(dir.join("memory.swap.max"), "4321\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let (m, buf) = (ctx.metrics(), ctx.measurements());
                assert_eq!(value(m, buf, "cgroup_memory_swap_current"), 1234);
                let max = points(m, buf, "cgroup_memory_swap_max");
                assert_eq!(max.len(), 1);
                assert_eq!(max[0].value.as_f64(), 4321.0);
            },
        );
        run_probe(&dir, runtime);
    }

    /// The pages swapped in between two polls are measured, in pages and in bytes.
    #[test]
    fn swap_in() {
        let (_root, dir) = cgroup_dir();
        let memory_stat = dir.join("memory.stat");
        std::fs::write(&memory_stat, "pswpin 10\n").unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(
                source(),
                || (),
                |ctx| {
                    // first poll: no previous value, hence no difference
                    assert_eq!(points(ctx.metrics(), ctx.measurements(), "cgroup_pswpin").len(), 0);
                },
            )
            .test_source(
                source(),
                move || std::fs::write(&memory_stat, "pswpin 15\n").unwrap(),
                |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    let page_size = rustix::param::page_size() as u64;
                    assert_eq!(value(m, buf, "cgroup_pswpin"), 15 - 10);
                    assert_eq!(value(m, buf, "cgroup_pswpin_bytes"), (15 - 10) * page_size);
                },
            );
        run_probe(&dir, runtime);
    }

    /// The pages swapped out between two polls are measured, in pages and in bytes.
    #[test]
    fn swap_out() {
        let (_root, dir) = cgroup_dir();
        let memory_stat = dir.join("memory.stat");
        std::fs::write(&memory_stat, "pswpout 20\n").unwrap();

        let runtime = RuntimeExpectations::new()
            .test_source(
                source(),
                || (),
                |ctx| {
                    // first poll: no previous value, hence no difference
                    assert_eq!(points(ctx.metrics(), ctx.measurements(), "cgroup_pswpout").len(), 0);
                },
            )
            .test_source(
                source(),
                move || std::fs::write(&memory_stat, "pswpout 30\n").unwrap(),
                |ctx| {
                    let (m, buf) = (ctx.metrics(), ctx.measurements());
                    let page_size = rustix::param::page_size() as u64;
                    assert_eq!(value(m, buf, "cgroup_pswpout"), 30 - 20);
                    assert_eq!(value(m, buf, "cgroup_pswpout_bytes"), (30 - 20) * page_size);
                },
            );
        run_probe(&dir, runtime);
    }

    /// The number of cpus available to the cgroup is measured, with the list of these cpus.
    #[test]
    fn cpuset() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("cpuset.cpus.effective"), "0-3\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let (m, buf) = (ctx.metrics(), ctx.measurements());
                assert_eq!(value(m, buf, "cpuset_cpus_effective"), 4);
                let point = points(m, buf, "cpuset_cpus_effective")[0];
                assert_eq!(attr(point, "core_list").as_deref(), Some("0,1,2,3"));
            },
        );
        run_probe(&dir, runtime);
    }

    /// Every point is measured for the cgroup, and has the common attributes of the probe.
    #[test]
    fn consumer_and_common_attrs() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("memory.current"), "5555\n").unwrap();
        std::fs::write(dir.join("cpuset.cpus.effective"), "0-3\n").unwrap();

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
