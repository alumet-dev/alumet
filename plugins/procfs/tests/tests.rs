//! Integration tests of the procfs plugin, which read the real `/proc` of the machine.

use std::time::Duration;

use alumet::{
    agent::{
        self,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::{MeasurementBuffer, MeasurementPoint},
    metrics::registry::MetricRegistry,
    pipeline::naming::SourceName,
    plugin::{PluginMetadata, rust::AlumetPlugin},
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};
use plugin_procfs::ProcfsPlugin;
use pretty_assertions::assert_eq;

const TIMEOUT: Duration = Duration::from_secs(5);

/// Runs an agent with the given plugins and expectations, until it stops.
fn run_agent(plugins: PluginSet, startup: StartupExpectations, runtime: RuntimeExpectations) {
    let agent = agent::Builder::new(plugins)
        .with_expectations(startup)
        .with_expectations(runtime)
        .build_and_start()
        .expect("agent should start");
    agent.wait_for_shutdown(TIMEOUT).expect("agent should stop");
}

/// Returns the value of the attribute `key`, as a string.
fn attr(point: &MeasurementPoint, key: &str) -> Option<String> {
    point.attributes().find(|(k, _)| *k == key).map(|(_, v)| v.to_string())
}

/// Returns the points of the metric named `metric`.
fn points<'a>(metrics: &MetricRegistry, buf: &'a MeasurementBuffer, metric: &str) -> Vec<&'a MeasurementPoint> {
    buf.iter()
        .filter(|p| metrics.by_id(&p.metric).unwrap().name == metric)
        .collect()
}

/// Default config of the plugin, modified by `edit`.
fn config(edit: impl FnOnce(&mut toml::Table)) -> toml::Table {
    let mut config = ProcfsPlugin::default_config().unwrap().unwrap().0;
    edit(&mut config);
    config
}

/// Disables the given sections of the config.
fn disable(config: &mut toml::Table, sections: &[&str]) {
    for section in sections {
        config[*section]["enabled"] = toml::Value::Boolean(false);
    }
}

fn plugin_set(config: toml::Table) -> PluginSet {
    let mut plugins = PluginSet::new();
    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<ProcfsPlugin>(),
        enabled: true,
        config: Some(config),
    });
    plugins
}

/// Only the memory source.
fn memory_only() -> toml::Table {
    config(|c| disable(c, &["kernel", "network", "processes"]))
}

fn source(name: &str) -> SourceName {
    SourceName::from_str("procfs", name)
}

/// The meminfo entries of the config are registered as snake_case metrics.
#[test]
fn memory_metrics() {
    let startup = StartupExpectations::new()
        .expect_metric::<u64>("mem_total", Unit::Byte)
        .expect_metric::<u64>("swap_cached", Unit::Byte)
        .expect_source("procfs", "memory");
    let runtime = RuntimeExpectations::new().test_source(source("memory"), || (), |_| ());
    run_agent(plugin_set(memory_only()), startup, runtime);
}

/// Polling the memory source measures each configured entry once.
#[test]
fn memory_values() {
    let runtime = RuntimeExpectations::new().test_source(
        source("memory"),
        || (),
        |ctx| assert_eq!(points(ctx.metrics(), ctx.measurements(), "mem_total").len(), 1),
    );
    run_agent(plugin_set(memory_only()), StartupExpectations::new(), runtime);
}

/// The kernel source pushes nothing on its first poll, then the cpu time per cpu state.
#[test]
fn kernel_source() {
    let config = config(|c| disable(c, &["memory", "network", "processes"]));
    let startup = StartupExpectations::new()
        .expect_metric::<u64>("kernel_cpu_time", PrefixedUnit::milli(Unit::Second))
        .expect_source("procfs", "kernel");
    let runtime = RuntimeExpectations::new()
        .test_source(source("kernel"), || (), |ctx| assert!(ctx.measurements().is_empty()))
        .test_source(
            source("kernel"),
            || (),
            |ctx| {
                let cpu_time = points(ctx.metrics(), ctx.measurements(), "kernel_cpu_time");
                assert!(!cpu_time.is_empty());
                assert!(cpu_time.iter().all(|p| attr(p, "cpu_state").is_some()));
            },
        );
    run_agent(plugin_set(config), startup, runtime);
}

/// The network source is created and pushes no delta on its first poll.
#[test]
fn network_source() {
    let config = config(|c| disable(c, &["kernel", "memory", "processes"]));
    let startup = StartupExpectations::new()
        .expect_metric::<u64>("network_bytes", Unit::Byte)
        .expect_source("procfs", "network");
    let runtime = RuntimeExpectations::new().test_source(
        source("network"),
        || (),
        |ctx| assert!(ctx.measurements().is_empty(), "no delta on the first poll"),
    );
    run_agent(plugin_set(config), startup, runtime);
}

/// In "event" mode, the process metrics are registered at startup.
#[test]
fn process_metrics() {
    let config = config(|c| {
        disable(c, &["kernel", "network"]);
        c["processes"]["strategy"] = toml::Value::String("event".into());
    });
    let startup = StartupExpectations::new()
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second))
        .expect_metric::<f64>("cpu_percent", Unit::Percent)
        .expect_metric::<u64>("memory_usage", Unit::Byte);
    let runtime = RuntimeExpectations::new().test_source(source("memory"), || (), |_| ());
    run_agent(plugin_set(config), startup, runtime);
}
