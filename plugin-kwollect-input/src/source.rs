use super::*;
use crate::Config;
use crate::kwollect::parse_measurements;
use alumet::{
    measurement::{MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::elements::{error::PollError, source::Source},
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use chrono::FixedOffset;
use log;
use std::time::{Duration, SystemTime};
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
                if let Some(measurements) = parse_measurements(data) {
                    for measure in measurements {
                        log::info!("MeasureKwollect: {:?}", measure); // To log measures of Kwollect
                        measurements.push(measure);
                    }
                }
            }
            Err(e) => log::error!("Failed to fetch data: {}", e),
        }

        Ok(())
    }
}
