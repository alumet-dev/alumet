use alumet::plugin::rust::AlumetPlugin;

use super::output_csv::CsvOutput;

pub struct DefaultPlugin;

impl AlumetPlugin for DefaultPlugin {
    fn name() -> &'static str {
        "default-plugin"
    }

    fn version() -> &'static str {
        "0.1.0"
    }
    
    fn init(config: &mut alumet::config::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(DefaultPlugin))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let output_file = "alumet-output.csv"; // todo use config
        let output = Box::new(CsvOutput::new(output_file, true)?);
        alumet.add_output(output);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
