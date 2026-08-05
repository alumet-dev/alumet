use std::thread;
use std::time::{Duration, Instant};

use alumet::{
        plugin::{
            ConfigTable,
            AlumetPluginStart,
            rust::{AlumetPlugin, deserialize_config, serialize_config},
        },
        measurement::{MeasurementBuffer, MeasurementPoint, Timestamp},
        units::Unit,
        resources::{ResourceConsumer},
};

use serde::{Deserialize, Serialize};

use lm_sensors::{Initializer, LMSensors};

use crate::{
    temperature_sensors::{SensorsFeature},
};

mod temperature_sensors;

pub struct TemperaturePlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct Config {
    // TODO: consider adding an option to use Fahrenheit degrees instead of Celsius
    
    /// Initial interval between two temperature measurements.
    #[serde(with = "humantime_serde")]
    pub poll_interval: Duration,

    /// Initial interval between two flushing of measurements.
    #[serde(with = "humantime_serde")]
    pub flush_interval: Duration,

    /// Get CPU Package temperature only.
    pub package_only: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(1), // 1Hz
            flush_interval: Duration::from_secs(5),
            package_only: false,
        }
    }
}


impl AlumetPlugin for TemperaturePlugin {
    fn name() -> &'static str {
        "temperature"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(TemperaturePlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // Create the metric
        let lm_metric = alumet.create_metric::<f64>(
            "temperature_lm_sensor",
            Unit::DegreeCelsius,
            "Temperature measured, as reported by lm_sensors."
        )?;

        // Get the config parameters out of 'self', to be used in the thread
        let poll_interval = self.config.poll_interval;
        let package_only = self.config.package_only;

        // A flush of the measurement buffer will be done after `flush_rounds` measurements
        let flush_rounds = ((self.config.flush_interval.as_nanos() / poll_interval.as_nanos()) as usize).max(1);
        
        // Create an autonomous source and add the source to the measurement pipeline
        alumet.add_autonomous_source_builder("temperature_autonomous_source", move |_ctx, cancel_token, tx| {
            // An new thread is needed because LMSensors and internal objects
            // are not Sync nor Send
            let handle = thread::spawn(move || {
                // Initialise all the things related to lm_sensors
                let lmsensors: LMSensors = Initializer::default().initialize().expect("Could not initialise LMSensors.");
                
                // This plugin only works for Intel processors for the moment
                // TODO: check the CPU vendor and stop if it is not Intel
                // TODO: consider adding support for AMD processors

                // Only getting temperature sensors from coretemp.
                let sensors_feature_list: Vec<SensorsFeature> = temperature_sensors::get_coretemp_feature_list(&lmsensors, package_only);
                log::info!("Got {} temperature sensor features", sensors_feature_list.len());
                
                let mut buf = MeasurementBuffer::new();
                let mut next_poll_time = Instant::now() + poll_interval;
                let mut round = 0;

                while !cancel_token.is_cancelled() {
                    // Get measurement from all temperature sensors
                    for feature in &sensors_feature_list {
                        let temperature = feature.read_temperature_value();
                        match temperature {
                            Ok(value) => {
                                buf.push(MeasurementPoint::new(
                                    Timestamp::now(),
                                    lm_metric,
                                    feature.resource.clone(),
                                    ResourceConsumer::LocalMachine,
                                    value
                                ));
                            }
                            Err(e) => {
                                log::warn!("Failed to get temperature from {}: {e}", &feature.label);
                            }
                        }
                    }
                    
                    round += 1;
                    if round == flush_rounds {
                        // Push and clear the buffer after at most the flush_interval period
                        let _ = tx.blocking_send(buf.clone());
                        buf.clear();
                        round = 0;
                    }

                    thread::sleep(next_poll_time.saturating_duration_since(Instant::now()));
                    next_poll_time += poll_interval;
                }

                // Flush one last time the buffer
                let _ = tx.blocking_send(buf.clone());
                buf.clear();
            });

            let source = Box::pin(async move {
                // Just wait for the thread to terminate
                handle.join().expect("Could not join on the temperature plugin thread");
                Ok(())
            });
            Ok(source)
        })?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
