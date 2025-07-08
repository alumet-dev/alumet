use super::*;
use crate::Config;
use crate::kwollect::parse_measurements;
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::elements::{error::PollError, source::Source},
    resources::{Resource, ResourceConsumer},
};
use chrono::FixedOffset;
use log;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

pub struct KwollectSource {
    config: Config,
    metric: TypedMetricId<f64>,
}

impl KwollectSource {
    pub fn new(config: Config, metric: TypedMetricId<f64>) -> Self {
        KwollectSource { config, metric }
    }
}

// See what exists for plugin-perf, plugin-procfs
impl Source for KwollectSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
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
                if let Some(parsed) = parse_measurements(data) {
                    for measure in parsed {
                        // Convert MeasureKwollect to MeasurementPoint
                        let value = match measure.value {
                            WrappedMeasurementValue::F64(v) => v,
                            WrappedMeasurementValue::U64(v) => v as f64,
                        };

                        // Create a measurement point and add it to the accumulator
                        let measurement_point = MeasurementPoint::new(
                            Timestamp::from(f64_to_system_time(measure.timestamp)),
                            self.metric,
                            Resource::LocalMachine,
                            ResourceConsumer::LocalMachine,
                            value,
                        );

                        measurements.push(measurement_point);
                    }
                }
            }
            Err(e) => log::error!("Failed to fetch data: {}", e),
        }
        Ok(())
    }
}

fn f64_to_system_time(seconds: f64) -> SystemTime {
    let secs = seconds as u64;
    let nanos = ((seconds - secs as f64) * 1_000_000_000.0) as u32;
    UNIX_EPOCH + std::time::Duration::new(secs, nanos)
}
