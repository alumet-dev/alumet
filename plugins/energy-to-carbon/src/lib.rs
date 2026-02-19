use alumet::plugin::{rust::AlumetPlugin, AlumetPluginStart, ConfigTable};

pub struct EnergyToCarbonPlugin;

impl AlumetPlugin for EnergyToCarbonPlugin {
    fn name() -> &'static str {
        "energy-to-carbon" // the name of your plugin, in lowercase, without the "plugin-" prefix
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION") // gets the version from the Cargo.toml of the plugin crate
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(None) // no config for the moment
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(EnergyToCarbonPlugin))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Hello!");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Bye!");
        Ok(())
    }
}