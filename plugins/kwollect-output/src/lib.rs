mod kwollect;
mod output;

use alumet::plugin::rust::{deserialize_config, serialize_config};
use alumet::plugin::{AlumetPluginStart, ConfigTable, rust::AlumetPlugin};
use serde::{Deserialize, Serialize};

use crate::output::KwollectOutput;

pub struct KwollectPlugin {
    config: Config,
}

impl AlumetPlugin for KwollectPlugin {
    fn name() -> &'static str {
        "kwollect-output"
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
        let output = Box::new(KwollectOutput::new(
            self.config.url.to_owned(),
            self.config.hostname.clone(),
            self.config.login.clone(),
            self.config.password.clone(),
            self.config.append_unit_to_metric_name,
            self.config.use_unit_display_name,
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
    pub hostname: Option<String>,
    pub append_unit_to_metric_name: bool,
    pub use_unit_display_name: bool,
}

fn default_client_name_and_site() -> (String, String) {
    let binding = hostname::get()
        .expect("No client_name specified in the config, and unable to retrieve the hostname of the current node.");
    let fullname = binding.to_string_lossy().to_string();
    // On Grid'5000 nodes have the following kind of hostname:
    // NODENAME.SITE.grid5000.fr
    // Let's retrieve only the nodename
    let parts: Vec<&str> = fullname.split('.').collect();
    if parts.len() >= 2 {
        (parts[0].to_string(), parts[1].to_string()) // first part of the hostname
    } else {
        (fullname, String::from("CHANGEME")) // Invalid format or SITE not recognized
    }
}

impl Default for Config {
    fn default() -> Self {
        let (hostname, site) = default_client_name_and_site();
        Self {
            url: format!("https://api.grid5000.fr/stable/sites/{site}/metrics"),
            hostname: Some(hostname),
            login: None,
            password: None,
            append_unit_to_metric_name: true,
            use_unit_display_name: true,
        }
    }
}
