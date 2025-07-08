use alumet::plugin::{
    AlumetPluginStart, ConfigTable,
    rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::time::{Duration, SystemTime};
use time::OffsetDateTime;

mod kwollect;
use kwollect::parse_measurements;

/// Configuration of input Kwollect plugin
pub struct KwollectPluginInput {
    config: Config,
}

/// Implementation of input Kwollect plugin as an alumet plugin
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

    // TODO: adding a source to the stop BUS --> we can try with start bus if we use sleep at the moment no?
    // TODO: Building response of the API as a csv? --> test with csv plugin
    // TODO: Erase the sleep and put start_alumet at the tsart and end_alumet at the end
    fn start(&mut self, _alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is starting");
        let start_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(start_alumet);
        let start_utc = convert_to_utc(system_time);

        std::thread::sleep(Duration::from_secs(10)); // to test the API

        let end_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(end_alumet);
        let end_utc = convert_to_utc(system_time);

        // Convert timestamp (UTC+2)
        let paris_offset = FixedOffset::east_opt(2 * 3600).unwrap();
        let start_paris = start_utc.with_timezone(&paris_offset);
        let end_paris = end_utc.with_timezone(&paris_offset);

        let url = build_kwollect_url(&self.config, &start_paris, &end_paris);

        match fetch_data(&url, &self.config) {
            Ok(data) => {
                log::info!("Raw API data: {:?}", data); // To log API data
                if let Some(measurements) = parse_measurements(data) {
                    for measure in measurements {
                        log::info!("MeasureKwollect: {:?}", measure); // To log measures of Kwollect
                    }
                }
            }
            Err(e) => log::error!("Failed to fetch data: {}", e),
        }

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is ending!");
        Ok(())
    }
}

fn convert_to_system_time(offset_date_time: OffsetDateTime) -> SystemTime {
    SystemTime::from(offset_date_time)
}

// Convert timestamp (UTC+2) to be able to put the good timestamp on API request to Grid'5000
fn convert_to_utc(system_time: SystemTime) -> DateTime<Utc> {
    system_time.into()
}

/// Constructs the API URL to query Kwollect by the Grid'5000 API
fn build_kwollect_url(config: &Config, start: &DateTime<FixedOffset>, end: &DateTime<FixedOffset>) -> String {
    format!(
        "https://api.grid5000.fr/stable/sites/{}/metrics?nodes={}&metrics={}&start_time={}&end_time={}",
        config.site,
        config.hostname,
        config.metrics,
        start.format("%Y-%m-%dT%H:%M:%S"),
        end.format("%Y-%m-%dT%H:%M:%S"),
    )
}

// Fetch data function based on https://docs.rs/reqwest/latest/reqwest/
/// Performs a synchronous HTTP GET request with basic authentication to the provided URL and returns the parsed JSON response.
fn fetch_data(url: &str, config: &Config) -> Result<Value, Box<dyn Error>> {
    log::info!("Fetching data from URL: {}", url);
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url)
        .basic_auth(&config.login, Some(&config.password))
        .send()?;

    let response_text = response.text()?;
    log::info!("Raw response: {}", response_text);
    let data: Value = serde_json::from_str(&response_text)?;
    Ok(data)
}

/// A structure that stocks the configuration parameters that are necessary to interact with grid'5000 API (to build the request)
#[derive(Serialize, Deserialize)]
pub struct Config {
    pub site: String,
    pub hostname: String,
    pub metrics: String,
    pub login: String,
    pub password: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            site: "lyon".to_string(),
            hostname: "taurus-7".to_string(),
            metrics: "wattmetre_power_watt".to_string(),
            // TO CHANGE
            login: "login".to_string(),
            password: "password".to_string(),
        }
    }
}
