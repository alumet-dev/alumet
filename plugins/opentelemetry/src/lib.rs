mod output;

use alumet::plugin::rust::{AlumetPlugin, deserialize_config, serialize_config};
use output::OpenTelemetryOutput;
use serde::{Deserialize, Serialize};

pub struct OpenTelemetryPlugin {
    config: Config,
}

impl AlumetPlugin for OpenTelemetryPlugin {
    fn name() -> &'static str {
        "opentelemetry"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let plugin_config: Config = deserialize_config(config)?;
        Ok(Box::new(OpenTelemetryPlugin { config: plugin_config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Create a new OpenTelemetryOutput instance
        let otel_output = Box::new(OpenTelemetryOutput::new(
            self.config.use_unit_display_name,
            self.config.add_attributes_to_labels,
            self.config.prefix.clone(),
            self.config.suffix.clone(),
            self.config.collector_host.clone(),
        )?);
        alumet.add_blocking_output("out", otel_output.clone())?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub collector_host: String,
    pub prefix: String,
    pub suffix: String,
    pub use_unit_display_name: bool,
    pub add_attributes_to_labels: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            collector_host: String::from("http://localhost:4317"),
            prefix: String::from(""),
            suffix: String::from("_alumet"),
            use_unit_display_name: true,
            add_attributes_to_labels: true,
        }
    }
}
