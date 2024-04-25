mod output;
// TODO mod input

use std::path::PathBuf;

use alumet::plugin::{rust::AlumetPlugin, ConfigTable};
use output::CsvOutput;

pub struct CsvPlugin {
    csv_path: PathBuf,
}

impl AlumetPlugin for CsvPlugin {
    fn name() -> &'static str {
        env!("CARGO_PKG_NAME")
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(_config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // TODO config options
        Ok(Box::new(CsvPlugin { csv_path: PathBuf::from("alumet-output.csv") }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let output = Box::new(CsvOutput::new(&self.csv_path, true)?);
        alumet.add_output(output);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
