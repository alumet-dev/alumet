use alumet::{
    metrics::TypedMetricId,
    pipeline::{
        control::{matching::SourceMatcher, request},
        elements::source::trigger::builder::ManualTriggerBuilder,
        naming::SourceName,
    },
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
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use time::OffsetDateTime;
use tokio::task;

mod kwollect;
mod source;

use crate::source::KwollectSource;

/// Structure for Kwollect implementation
pub struct KwollectPluginInput {
    config: Arc<Mutex<ParsedConfig>>,
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
        let config: Config = deserialize_config(config)?;
        let parsed_config = ParsedConfig {
            site: config.site,
            hostname: config.hostname,
            login: config.login,
            password: config.password,
            metrics: config.metrics,
            metric_ids: Vec::new(),
        };
        Ok(Box::new(KwollectPluginInput {
            config: Arc::new(Mutex::new(parsed_config)),
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is starting");

        // Create a metric for the source.
        let mut config = self.config.lock().unwrap();
        let mut metric_ids = Vec::with_capacity(config.metrics.len());

        for metric_name in &config.metrics {
            let kwollect_metric = alumet.create_metric::<f64>(
                metric_name,
                Unit::Watt,
                format!("Power consumption metric: {}", metric_name),
            )?;
            metric_ids.push(kwollect_metric);
        }

        config.metric_ids = metric_ids;
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

        let config_cloned = self.config.clone();
        let async_runtime = alumet.async_runtime().clone();

        event::end_consumer_measurement().subscribe(move |_evt| {
            log::debug!("End consumer measurement event received");

            let config = config_cloned.lock().unwrap();
            let pipeline_control = control_handle.clone();

            let end_alumet: OffsetDateTime = SystemTime::now().into();
            let system_time: SystemTime = convert_to_system_time(end_alumet);
            let end_utc = convert_to_utc(system_time);
            let end_paris = end_utc.with_timezone(&paris_offset);

            let config_for_url = Config {
                site: config.site.clone(),
                hostname: config.hostname.clone(),
                metrics: config.metrics.clone(),
                login: config.login.clone(),
                password: config.password.clone(),
            };

            let url = build_kwollect_url(&config_for_url, &start_paris, &end_paris);
            log::info!("API request should be triggered with URL: {}", url);

            let source = KwollectSource::new(config_for_url, config.metric_ids.clone(), url)
                .expect("Failed to create KwollectSource");
            let mut builder = ManualTriggerBuilder::new();
            let trigger_spec = builder.build().expect("Failed to build trigger");
            log::debug!("Creating request...");
            let request = request::create_one().add_source("kwollect_event_source", Box::new(source), trigger_spec);

            // Here we want the pipeline to wait for the reponse so we use block_on
            async_runtime
                .block_on(async {
                    let result = pipeline_control.send_wait(request, Duration::from_secs(1)).await;

                    if result.is_ok() {
                        log::debug!("Triggering Kwollect Source now");
                        let source_name =
                            SourceName::new("kwollect-input".to_string(), "kwollect_event_source".to_string());
                        let source_matcher = SourceMatcher::Name(source_name.into());
                        let trigger_now_request =
                            alumet::pipeline::control::request::source(source_matcher).trigger_now();
                        let trigger_result = pipeline_control
                            .send_wait(trigger_now_request, Duration::from_secs(1))
                            .await;
                        log::debug!("Trigger now result: {:?}", trigger_result);
                    }
                    result
                })
                .map_err(|e| {
                    log::error!("Error dispatching request: {:?}", e);
                    e
                })?;

            Ok(())
        });

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Kwollect-input plugin is ending!");
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
        config.metrics.join(","),
        start.format("%Y-%m-%dT%H:%M:%S"),
        end.format("%Y-%m-%dT%H:%M:%S"),
    )
}

// Fetch data function based on https://docs.rs/reqwest/latest/reqwest/
/// Performs a asynchronous HTTP GET request with basic authentication to the provided URL and returns the parsed JSON response.
async fn fetch_data_async(url: &str, config: &Config) -> Result<Value, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .basic_auth(&config.login, Some(&config.password))
        .send()
        .await?;

    let response_text = response.text().await?;
    let data: Value = serde_json::from_str(&response_text)?;
    Ok(data)
}

/// Here we ensure the pipeline waits for the response before proceeding.
/// Synchronous function that creates a Tokio runtime and uses block_on to run the asynchronous fetch_data_async function
/// Usefull because we need to integrate asynchronous operations into a synchronous context (exec.rs)
fn fetch_data(url: &str, config: &Config) -> Result<Value, Box<dyn Error>> {
    task::block_in_place(|| {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(fetch_data_async(url, config))
    })
}

/// A structure that stocks the configuration parameters that are necessary to interact with grid'5000 API (to build the request)
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    pub site: String,
    pub hostname: String,
    pub metrics: Vec<String>,
    pub login: String,
    pub password: String,
}

struct ParsedConfig {
    site: String,
    hostname: String,
    login: String,
    password: String,
    metrics: Vec<String>,
    metric_ids: Vec<TypedMetricId<f64>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            site: "lyon".to_string(),
            hostname: "taurus-7".to_string(),
            metrics: vec!["wattmetre_power_watt".to_string()],
            login: "login".to_string(),
            password: "password".to_string(),
        }
    }
}
