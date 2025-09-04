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

            let datetime = parse_timestamp(&measure.timestamp)?;
            let system: SystemTime = datetime.into();
            let timestamp = Timestamp::from(system);

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

/// Parses a timestamp string into a `DateTime<FixedOffset>`.
/// Supports multiple timestamp formats:
/// - Nanoseconds: `%Y-%m-%dT%H:%M:%S%.9f%:z`
/// - Microseconds: `%Y-%m-%dT%H:%M:%S%.6f%:z`
/// - Milliseconds: `%Y-%m-%dT%H:%M:%S%.3f%:z`
/// - No fractions: `%Y-%m-%dT%H:%M:%S%:z`
///
/// # Example
///
/// ``` ignore
/// use chrono::{DateTime, FixedOffset};
///
/// let timestamp = "2025-09-04T12:34:56.123456789+02:00";
/// let parsed: DateTime<FixedOffset> = parse_timestamp(timestamp).unwrap();
/// println!("Parsed: {}", parsed);
/// ```
pub fn parse_timestamp(timestamp: &str) -> anyhow::Result<DateTime<FixedOffset>> {
    let formats = [
        "%Y-%m-%dT%H:%M:%S%.9f%:z", // Nanoseconds
        "%Y-%m-%dT%H:%M:%S%.6f%:z", // Microseconds
        "%Y-%m-%dT%H:%M:%S%.3f%:z", // Milliseconds
        "%Y-%m-%dT%H:%M:%S%:z",     // No fractions
    ];
    formats
        .iter()
        .find_map(|format| DateTime::parse_from_str(timestamp, format).ok())
        .ok_or_else(|| anyhow::anyhow!("Failed to parse timestamp '{}': invalid format", timestamp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp() {
        // Test nanoseconds
        let timestamp_ns = "2025-09-04T12:34:56.123456789+02:00";
        let parsed_ns = parse_timestamp(timestamp_ns).unwrap();
        assert_eq!(parsed_ns.to_string(), "2025-09-04 12:34:56.123456789 +02:00");

        // Test microseconds
        let timestamp_us = "2025-09-04T12:34:56.123456+02:00";
        let parsed_us = parse_timestamp(timestamp_us).unwrap();
        assert_eq!(parsed_us.to_string(), "2025-09-04 12:34:56.123456 +02:00");

        // Test milliseconds
        let timestamp_ms = "2025-09-04T12:34:56.123+02:00";
        let parsed_ms = parse_timestamp(timestamp_ms).unwrap();
        assert_eq!(parsed_ms.to_string(), "2025-09-04 12:34:56.123 +02:00");

        // Test no fractions
        let timestamp_no_fraction = "2025-09-04T12:34:56+02:00";
        let parsed_no_fraction = parse_timestamp(timestamp_no_fraction).unwrap();
        assert_eq!(parsed_no_fraction.to_string(), "2025-09-04 12:34:56 +02:00");

        // Test invalid format
        let timestamp_invalid = "2025-09-04 12:34:56";
        let result = parse_timestamp(timestamp_invalid);
        assert!(result.is_err());
    }
}
