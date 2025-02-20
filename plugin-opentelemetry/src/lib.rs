mod output;

use alumet::plugin::rust::{deserialize_config, serialize_config, AlumetPlugin};
use output::OpentelemetryOutput;
use serde::{Deserialize, Serialize};

pub struct OpentelemetryPlugin {
    output: Box<OpentelemetryOutput>,
}

impl AlumetPlugin for OpentelemetryPlugin {
    fn name() -> &'static str {
        "plugin-opentelemetry"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        // Create a new OpentelemetryOutput instance
        let output = Box::new(OpentelemetryOutput::new(
            config.append_unit_to_metric_name,
            config.use_unit_display_name,
            config.add_attributes_to_labels,
            config.collector_host,
            config.prefix.clone(),
            config.suffix.clone(),
        )?);
        Ok(Box::new(OpentelemetryPlugin { output: output }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Add output for processing measurements
        alumet.add_blocking_output(self.output.clone());
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    collector_host: String,
    prefix: String,
    suffix: String,
    append_unit_to_metric_name: bool,
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            collector_host: String::from("http://localhost:4317"),
            prefix: String::from(""),
            suffix: String::from("_alumet"),
            append_unit_to_metric_name: true,
            use_unit_display_name: true,
            add_attributes_to_labels: true,
        }
    }
}
