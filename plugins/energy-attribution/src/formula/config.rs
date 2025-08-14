use alumet::measurement::AttributeValue;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PluginConfig {
    pub formulas: FxHashMap<String, FormulaConfig>,
}

/// Configuration for one attribution formula.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FormulaConfig {
    /// The mathematical expression to compute.
    /// The result must be in Joules.
    pub(super) expr: String,

    /// The reference timeseries, on which every other timeseries will be aligned.
    #[serde(rename = "ref")]
    pub reference_ident: String,

    /// Timeseries that will be grouped per resource.
    pub(super) per_resource: FxHashMap<String, FormulaTimeseriesConfig>,
    /// Timeseries that will be grouped per resource and per consumer.
    pub(super) per_consumer: FxHashMap<String, FormulaTimeseriesConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FormulaTimeseriesConfig {
    pub(super) metric: String,
    #[serde(flatten)]
    pub(super) filters: FilterConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FilterConfig {
    pub(super) resource_kind: Option<String>,
    pub(super) resource_id: Option<String>,

    pub(super) consumer_kind: Option<String>,
    pub(super) consumer_id: Option<String>,

    #[serde(flatten)]
    pub(super) attributes: FxHashMap<String, FilterAttributeValue>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(untagged)]
pub enum FilterAttributeValue {
    UInt(u64),
    Float(f64),
    Bool(bool),
    String(String),
}

impl FilterAttributeValue {
    pub fn matches(&self, value: &AttributeValue) -> bool {
        match (self, value) {
            (FilterAttributeValue::UInt(a), AttributeValue::U64(b)) => a == b,
            (FilterAttributeValue::Float(a), AttributeValue::F64(b)) => a == b,
            (FilterAttributeValue::Bool(a), AttributeValue::Bool(b)) => a == b,
            (FilterAttributeValue::String(a), AttributeValue::Str(b)) => a == b,
            (FilterAttributeValue::String(a), AttributeValue::String(b)) => a == b,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_config_cpu() {
        let config_cpu = r#"
            expr = "cpu_energy * cpu_usage"
            ref = "cpu_energy"

            [per_resource]
            cpu_energy = { metric = "rapl_consumed_energy", resource_kind = "local_machine", domain = "package_total" }

            [per_consumer]
            cpu_usage = { metric = "cpu_usage_percent" }
        "#;

        let config: FormulaConfig = toml::from_str(config_cpu).unwrap();
        assert_eq!(config.expr, "cpu_energy * cpu_usage");
        assert_eq!(config.per_resource["cpu_energy"].metric, "rapl_consumed_energy");
        assert_eq!(config.per_consumer["cpu_usage"].metric, "cpu_usage_percent");
    }

    #[test]
    fn test_config_gpu() {
        let config = r#"
            expr = "gpu_energy * (u_major * 0.8 + u_mem * 0.2)"
            ref = "gpu_energy"

            [per_resource]
            gpu_energy = { metric = "nvml_energy_consumption", resource_kind = "gpu" }

            [per_consumer]
            u_major = { metric = "nvml_gpu_utilization" }
            u_mem = { metric = "nvml_memory_utilization" }
        "#;

        let config: FormulaConfig = toml::from_str(config).unwrap();
        assert_eq!(config.expr, "gpu_energy * (u_major * 0.8 + u_mem * 0.2)");
        assert_eq!(config.per_resource["gpu_energy"].metric, "nvml_energy_consumption");
        assert_eq!(config.per_consumer["u_major"].metric, "nvml_gpu_utilization");
        assert_eq!(config.per_consumer["u_mem"].metric, "nvml_memory_utilization");
        println!("{}", toml::to_string_pretty(&PluginConfig{formulas: FxHashMap::from_iter([(String::from("a"), config)])}).unwrap());
    }

    #[test]
    fn test_plugin_config() {
        let config = r#"
            [formulas.attributed_energy_cpu]
            expr = "cpu_energy * cpu_usage"
            ref = "cpu_energy"

            [formulas.attributed_energy_cpu.per_resource]
            cpu_energy = { metric = "rapl_consumed_energy", resource_kind = "local_machine", domain = "package_total" }

            [formulas.attributed_energy_cpu.per_consumer]
            cpu_usage = { metric = "cpu_usage_percent" }

            [formulas.attributed_energy_gpu]
            expr = "gpu_energy * (u_major * 0.8 + u_mem * 0.2)"
            ref = "gpu_energy"

            [formulas.attributed_energy_gpu.per_resource]
            gpu_energy = { metric = "nvml_energy_consumption", resource_kind = "gpu" }

            [formulas.attributed_energy_gpu.per_consumer]
            u_major = { metric = "nvml_gpu_utilization" }
            u_mem = { metric = "nvml_memory_utilization" }
        "#;
        let mut config: PluginConfig = toml::from_str(config).unwrap();
        let cpu = config.formulas.remove("attributed_energy_cpu").unwrap();
        assert_eq!(cpu.expr, "cpu_energy * cpu_usage");
        assert_eq!(cpu.per_resource["cpu_energy"].metric, "rapl_consumed_energy");
        assert_eq!(cpu.per_consumer["cpu_usage"].metric, "cpu_usage_percent");

        let gpu = config.formulas.remove("attributed_energy_gpu").unwrap();
        assert_eq!(gpu.expr, "gpu_energy * (u_major * 0.8 + u_mem * 0.2)");
        assert_eq!(gpu.per_resource["gpu_energy"].metric, "nvml_energy_consumption");
        assert_eq!(gpu.per_consumer["u_major"].metric, "nvml_gpu_utilization");
        assert_eq!(gpu.per_consumer["u_mem"].metric, "nvml_memory_utilization");

        assert!(config.formulas.is_empty());
    }
}
