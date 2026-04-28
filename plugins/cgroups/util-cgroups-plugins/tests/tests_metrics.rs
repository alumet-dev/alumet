use alumet::{
    agent::{
        Builder,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::AttributeValue,
    pipeline::naming::SourceName,
    plugin::{AlumetPluginStart, ConfigTable, PluginMetadata, rust::AlumetPlugin},
    test::{RuntimeExpectations, StartupExpectations},
    units::{PrefixedUnit, Unit},
};
use std::time::Duration;
use util_cgroups_plugins::metrics::AugmentedMetrics;

const TIMEOUT: Duration = Duration::from_secs(1);
const PLUGIN_NAME: &str = "test-util-cgroups-plugin-metrics";
const SOURCE_NAME: &str = "test";

struct MockPlugin;

impl AlumetPlugin for MockPlugin {
    fn name() -> &'static str {
        PLUGIN_NAME
    }
    fn version() -> &'static str {
        "0.1.0"
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None)
    }

    fn init(_: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(Self))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let metrics = util_cgroups_plugins::metrics::Metrics::create(alumet)?;
        let _augmented_0 = util_cgroups_plugins::metrics::AugmentedMetric::with_attributes(
            metrics.memory_usage,
            vec![("attr".to_string(), AttributeValue::U64(2048))],
        );
        let _augmented_1 = AugmentedMetrics::no_additional_attribute(&metrics);
        let _augmented_2 = AugmentedMetrics::with_common_attr_slice(
            &metrics,
            &[("common_attr".to_string(), AttributeValue::U64(4096))],
        );

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn test_metrics_sets() {
    let mut plugins = PluginSet::new();

    plugins.add_plugin(PluginInfo {
        metadata: PluginMetadata::from_static::<MockPlugin>(),
        enabled: true,
        config: None,
    });

    let startup = StartupExpectations::new()
        .expect_metric::<u64>("cpu_time_delta", PrefixedUnit::nano(Unit::Second))
        .expect_metric::<f64>("cpu_percent", Unit::Percent)
        .expect_metric::<u64>("memory_usage", Unit::Byte)
        .expect_metric::<u64>("cgroup_memory_anonymous", Unit::Byte)
        .expect_metric::<u64>("cgroup_memory_file", Unit::Byte)
        .expect_metric::<u64>("cgroup_memory_kernel_stack", Unit::Byte)
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte)
        .expect_source(PLUGIN_NAME, SOURCE_NAME);

    let runtime = RuntimeExpectations::new().test_source(
        SourceName::from_str(PLUGIN_NAME, SOURCE_NAME),
        || {},
        |ctx| {
            let m = ctx.measurements();

            let cpu_time_delta = ctx.metrics().by_name("cpu_time_delta").unwrap().0;
            let cpu_percent = ctx.metrics().by_name("cpu_percent").unwrap().0;
            let memory_usage = ctx.metrics().by_name("memory_usage").unwrap().0;
            let cgroup_memory_anonymous = ctx.metrics().by_name("cgroup_memory_anonymous").unwrap().0;
            let cgroup_memory_file = ctx.metrics().by_name("cgroup_memory_file").unwrap().0;
            let cgroup_memory_kernel_stack = ctx.metrics().by_name("cgroup_memory_kernel_stack").unwrap().0;
            let cgroup_memory_pagetables = ctx.metrics().by_name("cgroup_memory_pagetables").unwrap().0;

            assert!(m.iter().any(|p| p.metric == cpu_time_delta));
            assert!(m.iter().any(|p| p.metric == cpu_percent));
            assert!(m.iter().any(|p| p.metric == memory_usage));
            assert!(m.iter().any(|p| p.metric == cgroup_memory_anonymous));
            assert!(m.iter().any(|p| p.metric == cgroup_memory_file));
            assert!(m.iter().any(|p| p.metric == cgroup_memory_kernel_stack));
            assert!(m.iter().any(|p| p.metric == cgroup_memory_pagetables));
        },
    );

    let agent = Builder::new(plugins)
        .with_expectations(startup)
        .with_expectations(runtime)
        .build_and_start()
        .unwrap();

    agent.wait_for_shutdown(TIMEOUT).unwrap();
}
