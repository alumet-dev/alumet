use alumet::plugin::rust::AlumetPlugin;

mod formula;
mod transform;

pub struct EnergyAttributionPlugin {
    // config: Config,
}

impl AlumetPlugin for EnergyAttributionPlugin {
    fn name() -> &'static str {
        todo!()
    }

    fn version() -> &'static str {
        todo!()
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        todo!()
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        todo!()
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}
