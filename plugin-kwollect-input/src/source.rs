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

pub struct KwollectSource {
    pub config: Config,
    pub metric: Vec<TypedMetricId<f64>>,
    pub url: String,
}

impl KwollectSource {
    pub fn new(config: Config, metric: Vec<TypedMetricId<f64>>, url: String) -> anyhow::Result<KwollectSource> {
        Ok(KwollectSource { config, metric, url })
    }
}

impl Source for KwollectSource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Polling KwollectSource");

        // To create a Measurement Point from the MeasureKwollect type data
        fn create_measurement_point(
            measure: &MeasureKwollect,
            metric: TypedMetricId<f64>,
            timestamp: Timestamp,
        ) -> Result<MeasurementPoint, PollError> {
            let resource = Resource::Custom {
                kind: Borrowed("device_id"),
                id: Owned(measure.device_id.to_string()),
            };
            let consumer = if let Some(AttributeValue::String(device_orig)) = measure.labels.get("_device_orig") {
                ResourceConsumer::Custom {
                    kind: Borrowed("device_origin"),
                    id: Owned(device_orig.to_string()),
                }
            } else {
                ResourceConsumer::LocalMachine
            };
            let metric_id = metric;
            let value = match measure.value {
                WrappedMeasurementValue::F64(v) => v,
                WrappedMeasurementValue::U64(v) => v as f64,
            };

            let measurement_point = MeasurementPoint::new(timestamp, metric_id, resource, consumer, value)
                .with_attr("metric_id", AttributeValue::String(measure.metric_id.clone()));

            Ok(measurement_point)
        }

        // Retrieve the URL stored in KwollectPluginInput
        match fetch_data(&self.url, &self.config) {
            Ok(data) => {
                log::debug!("Fetched data: {:?}", data);
                match parse_measurements(data) {
                    Ok(parsed) => {
                        log::debug!("Parsed measurements: {:?}", parsed);
                        for measure in parsed {
                            for &metric in &self.metric {
                                match create_measurement_point(&measure, metric, timestamp) {
                                    Ok(mp) => {
                                        log::debug!("Created measurement point: {:?}", mp);
                                        measurements.push(mp);
                                    }
                                    Err(e) => {
                                        log::error!("Error creating measurement point: {}", e);
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Parsing error: {}", e);
                        return Err(PollError::Fatal(anyhow::anyhow!("Failed to parse measurements")));
                    }
                }
            }
            Err(e) => {
                log::error!("Fetch error: {}", e);
                return Err(PollError::Fatal(anyhow::anyhow!("Failed to fetch data")));
            }
        }

        Ok(())
    }
}
