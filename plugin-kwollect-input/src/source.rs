use super::*;
use crate::kwollect::parse_measurements;
use crate::{Config, kwollect::MeasureKwollect};
use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::elements::{error::PollError, source::Source},
    resources::{Resource, ResourceConsumer},
};
use log;
use std::borrow::Cow::Borrowed;
use std::borrow::Cow::Owned;
use std::sync::Mutex;

pub struct KwollectSource {
    pub config: Config,
    pub metric: TypedMetricId<u64>,
    pub url: Arc<Mutex<Option<String>>>,
}

impl KwollectSource {
    pub fn new(
        config: Config,
        metric: TypedMetricId<u64>,
        url: Arc<Mutex<Option<String>>>,
    ) -> anyhow::Result<KwollectSource> {
        Ok(KwollectSource { config, metric, url })
    }
}

impl Source for KwollectSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Polling KwollectSource");

        // To create a Measurement Point from the MeasureKwollect type data
        fn create_measurement_point(
            measure: &MeasureKwollect,
            metric: TypedMetricId<u64>,
            timestamp: Timestamp,
        ) -> Result<MeasurementPoint, PollError> {
            let resource = Resource::Custom {
                kind: Borrowed("device_id"),
                id: Owned(measure.device_id.to_string()),
            };
            let consumer = ResourceConsumer::LocalMachine;
            let metric_id = metric;
            let value = match measure.value {
                WrappedMeasurementValue::F64(v) => v as u64,
                WrappedMeasurementValue::U64(v) => v,
            };

            let measurement_point = MeasurementPoint::new(timestamp, metric_id, resource, consumer, value)
                .with_attr("metric_id", AttributeValue::String(measure.metric_id.clone()));

            Ok(measurement_point)
        }

        // THERE IS ALSO A PROBLEM HERE BECAUSE OF THE SHARED URL
        let guard = self.url.try_lock();

        if let Ok(guard) = guard {
            if let Some(url) = &*guard {
                match fetch_data(url, &self.config) {
                    Ok(data) => {
                        log::info!("Fetched data: {:?}", data);
                        match parse_measurements(data) {
                            Ok(parsed) => {
                                for measure in parsed {
                                    match create_measurement_point(&measure, self.metric, timestamp) {
                                        Ok(mp) => measurements.push(mp),
                                        Err(e) => return Err(e),
                                    }
                                }
                                Ok(())
                            }
                            Err(e) => {
                                log::error!("Parsing error: {}", e);
                                Err(PollError::Fatal(anyhow::anyhow!("Failed to parse measurements")))
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Fetch error: {}", e);
                        Err(PollError::Fatal(anyhow::anyhow!("Failed to fetch data")))
                    }
                }
            } else {
                log::warn!("URL not set yet, skipping poll.");
                Ok(())
            }
        } else {
            log::warn!("Could not acquire lock on URL (maybe in use), skipping poll.");
            Ok(())
        }
    }
}
