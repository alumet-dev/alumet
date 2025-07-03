use alumet::{
    measurement::Timestamp,
    metrics,
    plugin::{
        AlumetPluginStart, ConfigTable,
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
};
use chrono::format::strftime::StrftimeItems;
use chrono::{DateTime, Utc};
mod output;
use serde::{Deserialize, Serialize};
use std::{
    thread::sleep,
    time::{Duration, SystemTime},
};
use time::{OffsetDateTime, format_description};

use crate::output::KwollectInput;

pub struct KwollectPluginInput {
    config: Config,
}

impl AlumetPlugin for KwollectPluginInput {
    fn name() -> &'static str {
        "kwollect-input"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(KwollectPluginInput { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is starting");
        let start_alumet: OffsetDateTime = SystemTime::now().into();
        sleep(Duration::from_secs(10));
        let end_alumet: OffsetDateTime = SystemTime::now().into();
        let url: String = build_kwollect_url(&self.config, &start_alumet, &end_alumet);
        let input = Box::new(KwollectInput::new(
            url,
            self.config.site.clone(),
            self.config.hostname.clone(),
            self.config.metrics.clone(),
        )?);
        alumet.add_blocking_output("kwollect-input", input)?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is ending!");
        Ok(())
    }
}

/// Parses an RFC 3339 date-and-time string into a Timestamp value.
//    pub(crate) fn timestamp_from_rfc3339(timestamp: &str) -> Timestamp {
//        SystemTime::from(OffsetDateTime::parse(timestamp, &Rfc3339).unwrap()).into()
//    }

fn build_kwollect_url(config: &Config, start_alumet: &OffsetDateTime, end_alumet: &OffsetDateTime) -> String {
    format!(
        "https://api.grid5000.fr/stable/sites/{}/metrics?nodes={}&metrics={}&start_time={}&end_time={}",
        config.site,
        config.hostname,
        config.metrics,
        start_alumet
            .format(&(format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second]").unwrap()))
            .unwrap(),
        end_alumet
            .format(&(format_description::parse("[year]-[month]-[day]T[hour]:[minute]:[second]").unwrap()))
            .unwrap(),
    )
}

#[derive(Serialize, Deserialize)]
struct Config {
    /// Site name
    site: String,
    /// Node or cluster name
    hostname: String, //maybe become a list of hostnames?
    // Metrics possibles
    metrics: String,
    // simply needs to be separated by commas: if do not put metrics, all of the are returned by default
    // https://api.grid5000.fr/doc/stable/#tag/metrics
}

fn default_client_name_and_site() -> (String, String, String) {
    let binding = hostname::get()
        .expect("No client_name specified in the config, and unable to retrieve the hostname of the current node.");
    let fullname = binding.to_string_lossy().to_string();
    let parts: Vec<&str> = fullname.split('.').collect();
    return (
        "lyon".to_string(),     //parts[0].to_string(),
        "taurus-7".to_string(), //parts[1].to_string(),
        "wattmetre_power_watt".to_string(),
    ); // first part of the hostname
}

impl Default for Config {
    fn default() -> Self {
        let (site, hostname, metrics) = default_client_name_and_site();
        Config {
            site,
            hostname,
            metrics,
        }
        // ajouter url ici?????
        //
    }
}
