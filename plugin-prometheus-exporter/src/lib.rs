mod output;

use alumet::plugin::rust::{deserialize_config, serialize_config, AlumetPlugin};
use output::PrometheusOutput;
use serde::{Deserialize, Serialize};

pub struct PrometheusPlugin {
    config: Config,
}

impl AlumetPlugin for PrometheusPlugin {
    fn name() -> &'static str {
        "prometheus-exporter"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(PrometheusPlugin { config: config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Create a new PrometheusOutput instance
        let output = Box::new(PrometheusOutput::new(
            self.config.append_unit_to_metric_name,
            self.config.use_unit_display_name,
            self.config.add_attributes_to_labels,
            self.config.port,
            self.config.host.clone(),
            self.config.prefix.clone(),
            self.config.suffix.clone(),
        )?);
        // Add output for processing measurements
        alumet.add_blocking_output(PrometheusPlugin::name(), output);

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    host: String,
    prefix: String,
    suffix: String,
    port: u16,
    append_unit_to_metric_name: bool,
    use_unit_display_name: bool,
    add_attributes_to_labels: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::from("0.0.0.0"),
            prefix: String::from(""),
            suffix: String::from("_alumet"),
            port: 9091,
            append_unit_to_metric_name: true,
            use_unit_display_name: true,
            add_attributes_to_labels: true,
        }
    }
}
