use alumet::{
    agent::{
        Builder,
        plugin::{PluginInfo, PluginSet},
    },
    measurement::AttributeValue,
    plugin::{AlumetPluginStart, ConfigTable, PluginMetadata, rust::AlumetPlugin},
    test::StartupExpectations,
    units::{PrefixedUnit, Unit},
};
use util_cgroups_plugins::metrics::AugmentedMetrics;

const PLUGIN_NAME: &str = "test-util-cgroups-plugin-metrics";

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
            metrics.cpu_time_delta,
            vec![("attr".to_string(), AttributeValue::U64(42))],
        );
        let _augmented_1 = AugmentedMetrics::no_additional_attribute(&metrics);
        let _augmented_2 =
            AugmentedMetrics::with_common_attr_slice(&metrics, &[("attr".to_string(), AttributeValue::U64(42))]);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[test]
fn test_metrics_are_registered() {
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
        .expect_metric::<u64>("cgroup_memory_pagetables", Unit::Byte);

    let agent = Builder::new(plugins)
        .with_expectations(startup)
        .build_and_start()
        .unwrap();

    drop(agent);
}
