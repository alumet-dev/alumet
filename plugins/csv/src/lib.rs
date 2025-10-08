mod csv;
mod output;
// TODO mod input

use std::path::PathBuf;

use alumet::plugin::{
    ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use output::CsvOutput;
use serde::{Deserialize, Serialize};

use crate::{csv::CsvParams, output::CsvOutputSettings};

pub struct CsvPlugin {
    config: Config,
}

impl AlumetPlugin for CsvPlugin {
    fn name() -> &'static str {
        "csv"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        Ok(Box::new(CsvPlugin { config }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let settings = CsvOutputSettings {
            force_flush: self.config.force_flush,
            append_unit_to_metric_name: self.config.append_unit_to_metric_name,
            use_unit_display_name: self.config.use_unit_display_name,
            params: CsvParams {
                delimiter: self.config.csv_delimiter,
                late_delimiter: self.config.csv_late_delimiter,
            },
        };
        let output = Box::new(CsvOutput::new(&self.config.output_path, settings)?);
        alumet.add_blocking_output("out", output)?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Absolute or relative path to the output_file
    pub output_path: PathBuf,
    /// Do we flush after each write (measurements)?
    pub force_flush: bool,
    /// Do we append the unit (unique name) to the metric name?
    pub append_unit_to_metric_name: bool,
    /// Do we use the unit display name (instead of its unique name)?
    pub use_unit_display_name: bool,
    /// The CSV delimiter, such as `;`
    pub csv_delimiter: char,
    /// The delimiter between the entries in `__late_attributes`.
    pub csv_late_delimiter: char,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("alumet-output.csv"),
            force_flush: true,
            use_unit_display_name: true,
            append_unit_to_metric_name: true,
            csv_delimiter: ';',
            csv_late_delimiter: ',',
        }
    }
}
