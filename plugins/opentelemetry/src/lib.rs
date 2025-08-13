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
            self.config.push_interval_seconds,
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
struct Config {
    collector_host: String,
    prefix: String,
    suffix: String,
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
    push_interval_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            collector_host: String::from("http://localhost:4317"),
            prefix: String::from(""),
            suffix: String::from("_alumet"),
            use_unit_display_name: true,
            add_attributes_to_labels: true,
            push_interval_seconds: 15,
        }
    }
}
