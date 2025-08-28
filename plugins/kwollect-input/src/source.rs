// This file implements the source functionality for the Kwollect input plugin.

use super::*;
use crate::kwollect::parse_measurements;
use crate::{Config, kwollect::MeasureKwollect};
use alumet::{
    measurement::{AttributeValue, MeasurementAccumulator, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::TypedMetricId,
    pipeline::elements::{error::PollError, source::Source},
    resources::{Resource, ResourceConsumer},
};
use chrono::DateTime;
use std::borrow::Cow::{Borrowed, Owned};
use std::time::SystemTime;

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
    fn poll(&mut self, measurements: &mut MeasurementAccumulator<'_>, _timestamp: Timestamp) -> Result<(), PollError> {
        log::info!("Polling KwollectSource");

        // Creates a Measurement Point from the MeasureKwollect type data
        fn create_measurement_point(
            measure: &MeasureKwollect,
            metric: TypedMetricId<f64>,
        ) -> anyhow::Result<MeasurementPoint> {
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

            let datetime = [
                "%Y-%m-%dT%H:%M:%S%.9f%:z", // Nanosecondes
                "%Y-%m-%dT%H:%M:%S%.6f%:z", // Microsecondes
                "%Y-%m-%dT%H:%M:%S%.3f%:z", // Millisecondes
                "%Y-%m-%dT%H:%M:%S%:z",     // Pas de fractions
            ]
            .iter()
            .find_map(|format| DateTime::parse_from_str(&measure.timestamp, format).ok())
            .ok_or_else(|| anyhow::anyhow!("Failed to parse datetime: invalid format"))?;

            // let datetime = DateTime::parse_from_str(&measure.timestamp, "%Y-%m-%dT%H:%M:%S.f%:z")
            //     .map_err(|e| anyhow::anyhow!("Failed to parse datetime: {}", e))?;
            let system_time: SystemTime = datetime.into();
            let timestamp = Timestamp::from(system_time);

            let measurement_point = MeasurementPoint::new(timestamp, metric_id, resource, consumer, value)
                .with_attr("metric_id", AttributeValue::String(measure.metric_id.clone()));

            Ok(measurement_point)
        }

        // Retrieve the URL stored in KwollectPluginInput
        let data = fetch_data(&self.url, &self.config)
            .map_err(|e| PollError::Fatal(anyhow::anyhow!("Failed to fetch data: {}", e)))?;
        log::debug!("Full API response: {data:?}");

        let parsed = parse_measurements(data)
            .map_err(|e| PollError::Fatal(anyhow::anyhow!("Failed to parse measurements: {}", e)))?;

        for measure in parsed {
            for &metric in &self.metric {
                match create_measurement_point(&measure, metric) {
                    Ok(mp) => {
                        log::debug!("Created measurement point: {mp:?}");
                        measurements.push(mp);
                    }
                    Err(e) => {
                        log::error!("Failed to create measurement point: {e}");
                        return Err(PollError::Fatal(anyhow::anyhow!(e)));
                    }
                }
            }
        }

        Ok(())
    }
}
