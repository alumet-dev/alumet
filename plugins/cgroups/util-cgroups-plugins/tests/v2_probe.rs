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
    pipeline::{elements::source::trigger::TriggerSpec, naming::SourceName},
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

/// Name of the probe source
fn source() -> SourceName {
    SourceName::from_str(TestPlugin::name(), SOURCE)
}

/// Creates an empty cgroup directory in a temporary root
fn cgroup_dir() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join(CGROUP_NAME);
    std::fs::create_dir(&dir).unwrap();
    (root, dir)
}

/// A cgroup without any file is accepted, and its probe measures nothing.
#[test]
fn empty_cgroup() {
    let (_root, dir) = cgroup_dir();
    let runtime = RuntimeExpectations::new().test_source(
        source(),
        || (),
        |ctx| {
            assert!(ctx.measurements().is_empty());
        },
    );
    run_probe(&dir, runtime);
}

/// A file that exists but cannot be read (here a directory) is an error, unlike a missing file.
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
    };
    use std::collections::BTreeMap;

    /// The value of the attribute `key`, as a string.
    fn attr(point: &MeasurementPoint, key: &str) -> Option<String> {
        point.attributes().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
    }

    /// The points of the buffer, by metric name, followed by `/kind` if the point has a "kind" attribute
    /// (e.g. `memory_usage` or `cpu_time_delta/user`). Sorted by key to be compared easily.
    /// Panics if two points have the same key.
    fn points_by_metric<'a>(
        metrics: &MetricRegistry,
        buf: &'a MeasurementBuffer,
    ) -> BTreeMap<String, &'a MeasurementPoint> {
        let mut map = BTreeMap::new();
        for p in buf.iter() {
            let name = &metrics.by_id(&p.metric).unwrap().name;
            let key = match attr(p, "kind") {
                Some(kind) => format!("{name}/{kind}"),
                None => name.clone(),
            };
            assert!(map.insert(key.clone(), p).is_none(), "duplicate point for {key}");
        }
        map
    }

    /// The memory used by the cgroup and its limit are measured in bytes.
    #[test]
    fn memory() {
        let (_root, dir) = cgroup_dir();
        std::fs::write(dir.join("memory.current"), "5555\n").unwrap();
        std::fs::write(dir.join("memory.max"), "8888\n").unwrap();

        let runtime = RuntimeExpectations::new().test_source(
            source(),
            || (),
            |ctx| {
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert_eq!(p.keys().collect::<Vec<_>>(), ["memory_max", "memory_usage"]);
                assert_eq!(p["memory_usage"].value.as_u64(), 5555);
                assert_eq!(p["memory_max"].value.as_f64(), 8888.0);
            },
        );
        run_probe(&dir, runtime);
    }

    /// The cpu time is a difference between two polls: nothing is measured on the first one.
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
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert!(p.is_empty(), "nothing expected, got {:?}", p.keys());
            },
        );
        run_probe(&dir, runtime);
    }

    /// The cpu time consumed between two polls is measured in ns (cpu.stat is in µs), for each kind.
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
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    // nothing else than the cpu metrics is measured
                    assert_eq!(
                        p.keys().collect::<Vec<_>>(),
                        [
                            "cpu_percent/system",
                            "cpu_percent/total",
                            "cpu_percent/user",
                            "cpu_time_delta/system",
                            "cpu_time_delta/total",
                            "cpu_time_delta/user",
                        ]
                    );
                    assert_eq!(p["cpu_time_delta/total"].value.as_u64(), 3_000_000);
                    assert_eq!(p["cpu_time_delta/user"].value.as_u64(), 1_300_000);
                    assert_eq!(p["cpu_time_delta/system"].value.as_u64(), 1_700_000);
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
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    let percent = |kind: &str| p[&format!("cpu_percent/{kind}")].value.as_f64();
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
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert!(p.is_empty(), "nothing expected, got {:?}", p.keys());
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
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    assert_eq!(
                        p.keys().collect::<Vec<_>>(),
                        ["io_pressure_full_total", "io_pressure_some_total"]
                    );
                    assert_eq!(p["io_pressure_some_total"].value.as_u64(), 4000 - 1000);
                    assert_eq!(p["io_pressure_full_total"].value.as_u64(), 2500 - 500);
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
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert_eq!(
                    p.keys().collect::<Vec<_>>(),
                    [
                        "cgroup_memory_anonymous",
                        "cgroup_memory_file",
                        "cgroup_memory_kernel_stack",
                        "cgroup_memory_pagetables",
                        "cgroup_slab_reclaimable",
                    ]
                );
                assert_eq!(p["cgroup_memory_anonymous"].value.as_u64(), 1111);
                assert_eq!(p["cgroup_memory_file"].value.as_u64(), 2222);
                assert_eq!(p["cgroup_memory_kernel_stack"].value.as_u64(), 3333);
                assert_eq!(p["cgroup_memory_pagetables"].value.as_u64(), 4444);
                assert_eq!(p["cgroup_slab_reclaimable"].value.as_u64(), 5555);
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
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert_eq!(
                    p.keys().collect::<Vec<_>>(),
                    ["cgroup_memory_swap_current", "cgroup_memory_swap_max"]
                );
                assert_eq!(p["cgroup_memory_swap_current"].value.as_u64(), 1234);
                assert_eq!(p["cgroup_memory_swap_max"].value.as_f64(), 4321.0);
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
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    assert!(p.is_empty(), "nothing expected, got {:?}", p.keys());
                },
            )
            .test_source(
                source(),
                move || std::fs::write(&memory_stat, "pswpin 15\n").unwrap(),
                |ctx| {
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    assert_eq!(p.keys().collect::<Vec<_>>(), ["cgroup_pswpin", "cgroup_pswpin_bytes"]);
                    let page_size = rustix::param::page_size() as u64;
                    assert_eq!(p["cgroup_pswpin"].value.as_u64(), 15 - 10);
                    assert_eq!(p["cgroup_pswpin_bytes"].value.as_u64(), (15 - 10) * page_size);
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
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    assert!(p.is_empty(), "nothing expected, got {:?}", p.keys());
                },
            )
            .test_source(
                source(),
                move || std::fs::write(&memory_stat, "pswpout 30\n").unwrap(),
                |ctx| {
                    let p = points_by_metric(ctx.metrics(), ctx.measurements());
                    assert_eq!(p.keys().collect::<Vec<_>>(), ["cgroup_pswpout", "cgroup_pswpout_bytes"]);
                    let page_size = rustix::param::page_size() as u64;
                    assert_eq!(p["cgroup_pswpout"].value.as_u64(), 30 - 20);
                    assert_eq!(p["cgroup_pswpout_bytes"].value.as_u64(), (30 - 20) * page_size);
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
                let p = points_by_metric(ctx.metrics(), ctx.measurements());
                assert_eq!(p.keys().collect::<Vec<_>>(), ["cpuset_cpus_effective"]);
                let point = p["cpuset_cpus_effective"];
                assert_eq!(point.value.as_u64(), 4);
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
