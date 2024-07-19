use alumet::plugin::rust::{deserialize_config, serialize_config, AlumetPlugin};
use serde::{Deserialize, Serialize};

mod system;
mod process;

pub struct ProcfsPlugin {}

impl AlumetPlugin for ProcfsPlugin {
    fn name() -> &'static str {
        "procfs"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
    
    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;
        
        todo!()
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        todo!()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        todo!()
    }
}

#[derive(Serialize, Deserialize)]
struct Config {
    
}

impl Default for Config {
    fn default() -> Self {
        Self {  }
    }
}
