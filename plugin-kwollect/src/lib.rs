mod kwollect;
mod output;

use alumet::plugin::rust::{deserialize_config, serialize_config};
use alumet::plugin::{rust::AlumetPlugin, AlumetPluginStart, ConfigTable};
use serde::{Deserialize, Serialize};

use crate::output::KwollectOutput;

pub struct KwollectPlugin {
    config: Config,
}

impl AlumetPlugin for KwollectPlugin {
    fn name() -> &'static str {
        "kwollect"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(KwollectPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let _url = &self.config.url;
        let login = self.config.login.clone();
        let password = self.config.password.clone();
        let output = Box::new(KwollectOutput::new(
            self.config.url.to_string(),
            self.config.hostname.clone(),
            login,
            password,
        )?);
        alumet.add_blocking_output("kwollect-output", output)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub login: Option<String>,
    pub password: Option<String>,
    pub hostname: String,
}

fn default_client_name() -> String {
    let binding = hostname::get()
        .expect("No client_name specified in the config, and unable to retrieve the hostname of the current node.");
    let fullname = binding.to_string_lossy().to_string();
    let sites = [
        "lyon",
        "grenoble",
        "lille",
        "louvain",
        "luxembourg",
        "nancy",
        "nantes",
        "rennes",
        "sophia",
        "strasbourg",
        "toulouse",
    ];

    // On Grid'5000 nodes have the following kind of hostname:
    // NODENAME.SITE.grid5000.fr
    // Let's retrieve only the nodename
    let parts: Vec<&str> = fullname.split('.').collect();
    if parts.len() >= 2 && sites.contains(&parts[1]) {
        parts[0].to_string() // first part of the hostname
    } else {
        fullname // Invalid format or SITE not recognized
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: String::from("http://localhost:8080"),
            hostname: default_client_name(),
            login: None,
            password: None,
        }
    }
}
