use alumet::{
    pipeline::{elements::source::trigger::builder::ManualTriggerBuilder, naming::SourceName},
    plugin::{
        AlumetPluginStart, AlumetPostStart, ConfigTable,
        event::{self},
        rust::{AlumetPlugin, deserialize_config, serialize_config},
    },
    units::Unit,
};
use chrono::{DateTime, FixedOffset, Utc};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use std::{error::Error, sync::Arc};
use time::OffsetDateTime;

mod kwollect;
mod source;
use crate::source::KwollectSource;

/// Structure for Kwollect implementation
pub struct KwollectPluginInput {
    config: Config,
    shared_url: Option<Arc<Mutex<Option<String>>>>,
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
        Ok(Box::new(KwollectPluginInput {
            config,
            shared_url: None,
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is starting");

        // Create a metric for the source.
        let kwollect_metric = alumet.create_metric::<u64>(
            "power_consumption",
            Unit::Watt,
            "Power consumption of the node reported by wattmetre, in watt ",
        )?;

        let shared_url = Arc::new(Mutex::new(None));
        self.shared_url = Some(shared_url.clone());
        let source = KwollectSource::new(self.config.clone(), kwollect_metric, shared_url.clone())?;

        let trigger_spec = ManualTriggerBuilder::new().build()?;

        alumet.add_source("kwollect_event_source", Box::new(source), trigger_spec)?;
        Ok(())
    }

    // Here this is where we want to call the API
    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        let paris_offset = FixedOffset::east_opt(2 * 3600).unwrap();
        let start_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(start_alumet);
        let start_utc = convert_to_utc(system_time);
        let start_paris = start_utc.with_timezone(&paris_offset);
        let config = self.config.clone();
        let control_handle_clone = control_handle.clone();
        let url_handle = self.shared_url.clone().expect("shared_url not initialized");

        let async_runtime = alumet.async_runtime();

        event::end_consumer_measurement().subscribe(move |_evt| {
            log::info!("End consumer measurement event received");
            let end_alumet: OffsetDateTime = SystemTime::now().into();
            let system_time: SystemTime = convert_to_system_time(end_alumet);
            let end_utc = convert_to_utc(system_time);
            let end_paris = end_utc.with_timezone(&paris_offset);
            let url = build_kwollect_url(&config, &start_paris, &end_paris);
            log::info!("API request should be triggered with URL: {}", url);

            // THE PROBLEM COMES FROM THIS SHARED URL
            let url_handle_clone = url_handle.clone();
            async_runtime.spawn(async move {
                let mut guard = url_handle_clone.lock().unwrap();
                *guard = Some(url);
            });

            let control_handle = control_handle_clone.clone();
            let task = async_runtime.spawn(async move {
                match control_handle
                    .send_wait(
                        alumet::pipeline::control::request::source(SourceName::from_str(
                            "kwollect-input",
                            "kwollect_event_source",
                        ))
                        .trigger_now(),
                        Duration::from_secs(1),
                    )
                    .await
                {
                    Ok(_) => log::info!("Successfully triggered source"),
                    Err(e) => log::error!("Failed to trigger source: {}", e),
                }
            });

            async_runtime.block_on(async {
                if let Err(e) = task.await {
                    log::error!("Task failed: {:?}", e);
                }
            });

            Ok(())
        });

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
#[derive(Serialize, Deserialize, Clone)]
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
