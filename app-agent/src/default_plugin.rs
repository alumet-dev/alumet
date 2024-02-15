use super::output_csv::CsvOutput;

pub struct DefaultPlugin;

impl alumet::plugin::Plugin for DefaultPlugin {
    fn name(&self) -> &str {
        "default-plugin"
    }

    fn version(&self) -> &str {
        "0.1.0"
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
