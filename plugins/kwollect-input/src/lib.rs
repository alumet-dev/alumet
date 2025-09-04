// This file contains the main implementation of the Kwollect input plugin for Alumet.

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
    units::{PrefixedUnit, Unit, UnitPrefix},
};
use anyhow::Context;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, SystemTime};
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};
use time::OffsetDateTime;

mod kwollect;
mod source;

use crate::source::KwollectSource;

/// Structure for Kwollect implementation
pub struct KwollectPluginInput {
    config: Arc<Mutex<ParsedConfig>>,
}

/// Implementation of input Kwollect plugin as an Alumet plugin
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
            utc_offset: config.utc_offset,
            metrics: config.metrics,
            metric_ids: Vec::new(),
        };
        Ok(Box::new(KwollectPluginInput {
            config: Arc::new(Mutex::new(parsed_config)),
        }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        log::info!("Kwollect-input plugin is starting");

        // Create metric(s) for the source
        let mut config = self.config.lock().unwrap();
        let mut metric_ids = Vec::with_capacity(config.metrics.len());

        for metric_name in &config.metrics {
            let unit_str = extract_unit_from_metric_name(metric_name);
            let prefixed_unit = if let Ok(unit) = PrefixedUnit::from_str(&unit_str) {
                unit
            } else if let Ok(base_unit) = Unit::from_str(&unit_str) {
                PrefixedUnit {
                    base_unit,
                    prefix: UnitPrefix::Plain,
                }
            } else {
                // fallback: create a custom unit if it doesn't exists
                PrefixedUnit {
                    base_unit: Unit::Custom {
                        unique_name: unit_str.clone(),
                        display_name: unit_str.clone(),
                    },
                    prefix: UnitPrefix::Plain,
                }
            };

            let kwollect_metric = alumet
                .create_metric::<f64>(
                    metric_name,
                    prefixed_unit, // Base unit for Alumet
                    format!("Metric: {metric_name}"),
                )
                .expect("Failed to create metric");

            metric_ids.push(kwollect_metric);
        }

        config.metric_ids = metric_ids;
        Ok(())
    }

    /// This function sets up a subscription to react when a consumer measurement event ends. When triggered, it:
    /// 1. Records the start and end times of the Alumet pipeline. Kwollect expects timestamps in Paris timezone
    ///    (UTC+2) so we converted it.
    /// 2. Builds and sends a request to KwollectSource using these timestamps. The pipeline waits for a response
    ///    (timeout: 5 seconds) to ensure the source is registered before proceeding.
    /// 3. Ensures the pipeline waits for the source’s response before completing, using Alumet’s
    ///    async_runtime and block_on. The Alumet pipeline uses an asynchronous runtime for concurrency, and this
    ///    function needs to block the pipeline until the Kwollect request is processed so async_runtime.block_on
    ///    runs the async code (request sending and triggering) synchronously, forcing the pipeline to wait for
    ///    completion.
    fn post_pipeline_start(&mut self, alumet: &mut AlumetPostStart) -> anyhow::Result<()> {
        let control_handle = alumet.pipeline_control();
        let config_cloned = self.config.clone();
        let async_runtime = alumet.async_runtime().clone();

        let start_alumet: OffsetDateTime = SystemTime::now().into();
        let system_time: SystemTime = convert_to_system_time(start_alumet);
        let start_utc = convert_to_utc(system_time);
        let paris_offset = if let Some(hours) = config_cloned.lock().unwrap().utc_offset {
            FixedOffset::east_opt(hours * 3600).unwrap()
        } else {
            FixedOffset::east_opt(0).unwrap() // fallback : UTC
        };
        let start_paris = start_utc.with_timezone(&paris_offset);
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
                utc_offset: config.utc_offset,
            };

            let url = build_kwollect_url(&config_for_url, &start_paris, &end_paris);
            log::info!("API request should be triggered with URL: {url}");

            let source = KwollectSource::new(config_for_url, config.metric_ids.clone(), url)
                .expect("Failed to create KwollectSource");

            let mut builder = ManualTriggerBuilder::new();
            let trigger_spec = builder.build().expect("Failed to build trigger");
            log::debug!("Creating request...");

            let request = request::create_one().add_source("kwollect_event_source", Box::new(source), trigger_spec);

            // The pipeline will wait for the response of the source
            async_runtime
                .block_on(async {
                    let result = pipeline_control.send_wait(request, Duration::from_secs(5)).await;
                    match &result {
                        Ok(_) => {
                            log::debug!("Request registered successfully: source added.");
                        }
                        Err(e) => {
                            log::error!("Failed to register request (add_source): {e:?}");
                        }
                    }

                    if result.is_ok() {
                        log::debug!("Triggering Kwollect Source now");
                        let source_name =
                            SourceName::new("kwollect-input".to_string(), "kwollect_event_source".to_string());
                        let source_matcher = SourceMatcher::Name(source_name.into());
                        let trigger_now_request =
                            alumet::pipeline::control::request::source(source_matcher).trigger_now();
                        let trigger_result = pipeline_control
                            .send_wait(trigger_now_request, Duration::from_secs(5))
                            .await;
                        match &trigger_result {
                            Ok(_) => log::debug!("Triggered Kwollect source."),
                            Err(e) => log::error!("Failed to trigger source: {e:?}"),
                        }
                    }
                    result
                })
                .map_err(|e| {
                    log::error!("Error dispatching request: {e:?}");
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

/// Normalizes a unit string to the UCUM standard if not already done by Alumet Core.
/// Note: Some metrics are not enabled for Prometheus exporter:
///     `prom_all_metrics`, `prom_default_metrics`, `prom_nvgpu_all_metrics`,
///     `prom_nvgpu_default_metrics`.
///     Use the Prometheus exporter plugin instead for these metrics.
fn normalize_unit(unit_str: &str) -> String {
    match unit_str {
        "watt" => "W".to_string(),           // UCUM standard
        "kilowatt" => "kW".to_string(),      // UCUM standard
        "watthour" => "W.h".to_string(),     // UCUM standard
        "celsius" => "Cel".to_string(),      // UCUM standard
        "percent" | "%" => "%".to_string(),  // UCUM standard
        "VA" => "V.A".to_string(),           // Not UCUM standard (Volt-Ampere)
        "VAr" => "V.A_r".to_string(),        // Not UCUM standard (Reactive Volt-Ampere)
        "amp" | "ampere" => "A".to_string(), // UCUM standard
        "volt" => "V".to_string(),           // UCUM standard
        "hertz" => "Hz".to_string(),         // UCUM standard
        "lux" => "lx".to_string(),           // UCUM standard
        "bar" => "bar".to_string(),          // UCUM standard
        "deg" => "°".to_string(),            // UCUM standard
        "m/s" => "m/s".to_string(),          // UCUM standard
        "rpm" => "rpm".to_string(),          // Not UCUM standard
        "cfm" => "[cft_i]/min".to_string(),  // UCUM standard (cubic foot per minute)
        "bytes" => "By".to_string(),         // UCUM standard
        "packets" => "tot.".to_string(),     // UCUM standard for a count
        _ => unit_str.to_string(),           // Return as-is if already UCUM or unknown
    }
}

/// Extracts the unit from a metric name.
/// The unit is typically the last segment of the metric name, unless the name ends with "total".
/// In that case, the unit is the segment before "total", or the segment before "discard" or "error" if present.
fn extract_unit_from_metric_name(metric_name: &str) -> String {
    let parts: Vec<&str> = metric_name.split('_').collect();
    // Special handling if the last part is "total"
    if parts.len() >= 3
        && let Some(last_segment) = parts.last()
        && last_segment == &"total"
    {
        let second_last_segment = parts[parts.len() - 2];
        if second_last_segment == "discard" || second_last_segment == "error" {
            let unit_index = parts.len() - 3;
            let unit = parts[unit_index];
            return normalize_unit(unit);
        } else {
            let unit = parts[parts.len() - 2];
            return normalize_unit(unit);
        }
    }
    // Default: last segment is the unit
    let unit_str = parts.last().expect("Metric name cannot be empty");
    normalize_unit(unit_str)
}

fn convert_to_system_time(offset_date_time: OffsetDateTime) -> SystemTime {
    SystemTime::from(offset_date_time)
}

// Convert timestamp (UTC+2) to be able to set the correct timestamp on API request to Grid'5000
fn convert_to_utc(system_time: SystemTime) -> DateTime<Utc> {
    system_time.into()
}

/// Constructs the API URL to query Kwollect via the Grid'5000 API
fn build_kwollect_url(config: &Config, start: &DateTime<FixedOffset>, end: &DateTime<FixedOffset>) -> String {
    format!(
        "https://api.grid5000.fr/stable/sites/{}/metrics?nodes={}&metrics={}&start_time={}&end_time={}",
        config.site,
        config.hostname,
        config.metrics.join(","),
        start.timestamp(),
        end.timestamp(),
    )
}

/// Performs an asynchronous HTTP GET request with basic authentication to the provided URL and returns the parsed JSON response.
fn fetch_data(url: &str, config: &Config) -> Result<Value, anyhow::Error> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(url)
        .basic_auth(&config.login, Some(&config.password))
        .send()
        .context("Failed to send HTTP request")?;
    let response_text = response.text().context("Failed to read response text")?;
    let data: Value = serde_json::from_str(&response_text).context("Failed to parse JSON")?;
    Ok(data)
}

/// A structure that stores the configuration parameters necessary to interact with the Grid'5000 API (to build the request)
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    pub site: String,
    pub hostname: String,
    pub metrics: Vec<String>,
    pub login: String,
    pub password: String,
    pub utc_offset: Option<i32>,
}

struct ParsedConfig {
    site: String,
    hostname: String,
    login: String,
    password: String,
    utc_offset: Option<i32>,
    metrics: Vec<String>,
    metric_ids: Vec<TypedMetricId<f64>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            site: "cluster".to_string(),
            hostname: "node".to_string(),
            metrics: vec!["metric".to_string()],
            login: "login".to_string(),
            password: "password".to_string(),
            utc_offset: Some(2), // UTC+2 (CEST, Central European Summer Time; note: UTC+1/CET applies in winter)
        }
    }
}
