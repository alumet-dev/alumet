//! This module provides functionality to serialize and deserialize measurement data for Kwollect.

use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
use anyhow::Context;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;

/// A structure to represent a measure collected by Kwollect.
#[derive(Debug)]
pub struct MeasureKwollect {
    pub device_id: String,
    pub labels: HashMap<String, AttributeValue>,
    pub metric_id: String,
    pub timestamp: String,
    pub value: WrappedMeasurementValue,
}

/// Implements serialization for MeasureKwollect which allows MeasureKwollect instances to be converted into a JSON-like map format.
impl Serialize for MeasureKwollect {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("timestamp", &self.timestamp)?;
        map.serialize_entry("metric_id", &self.metric_id)?;
        map.serialize_entry("device_id", &self.device_id)?;

        match self.value {
            WrappedMeasurementValue::F64(v) => map.serialize_entry("value", &v)?,
            WrappedMeasurementValue::U64(v) => map.serialize_entry("value", &v)?,
        };

        struct LabelsSerializer<'a>(&'a HashMap<String, AttributeValue>);

        impl Serialize for LabelsSerializer<'_> {
            fn serialize<T>(&self, serializer: T) -> Result<T::Ok, T::Error>
            where
                T: serde::Serializer,
            {
                let mut labels_map = serializer.serialize_map(Some(self.0.len()))?;
                for (key, value) in self.0 {
                    match value {
                        AttributeValue::Bool(v) => labels_map.serialize_entry(key, v)?,
                        AttributeValue::F64(v) => labels_map.serialize_entry(key, v)?,
                        AttributeValue::U64(v) => labels_map.serialize_entry(key, v)?,
                        AttributeValue::Str(v) => labels_map.serialize_entry(key, v)?,
                        AttributeValue::String(v) => labels_map.serialize_entry(key, v)?,
                    }
                }
                labels_map.end()
            }
        }

        map.serialize_entry("labels", &LabelsSerializer(&self.labels))?;
        map.end()
    }
}

/// Visitor for deserializing MeasureKwollect from JSON that guides the deserialization process by defining how to interpret each field.
struct MeasureKwollectVisitor;

impl<'de> Visitor<'de> for MeasureKwollectVisitor {
    type Value = MeasureKwollect;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map representing a MeasureKwollect")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut device_id = None;
        let mut labels = None;
        let mut metric_id = None;
        let mut timestamp = None;
        let mut value = None;

        while let Some(key) = access.next_key::<String>()? {
            match key.as_str() {
                "device_id" => {
                    if device_id.is_none() {
                        device_id = Some(access.next_value()?);
                    }
                }
                // labels is a HashMap<String, AttributeValue>: deserialize for each type of AttributeValue
                "labels" => {
                    if labels.is_none() {
                        let label_map: Map<String, Value> = access.next_value()?;
                        let mut labels_map = HashMap::new();
                        for (k, v) in label_map {
                            let attribute_value = match v {
                                Value::Bool(b) => Ok(AttributeValue::Bool(b)),
                                Value::Number(n) if n.is_f64() => Ok(AttributeValue::F64(n.as_f64().unwrap())),
                                Value::Number(n) if n.is_u64() => Ok(AttributeValue::U64(n.as_u64().unwrap())),
                                Value::Number(n) if n.is_i64() => Ok(AttributeValue::U64(n.as_i64().unwrap() as u64)),
                                Value::String(s) => Ok(AttributeValue::String(s)),
                                Value::Array(arr) => {
                                    // Convert array to a string representation
                                    let array_as_string =
                                        arr.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                                    Ok(AttributeValue::String(array_as_string))
                                }
                                _ => Err(de::Error::custom("Unsupported value type")),
                            }?;
                            labels_map.insert(k, attribute_value);
                        }
                        labels = Some(labels_map);
                    }
                }
                "metric_id" => {
                    if metric_id.is_none() {
                        metric_id = Some(access.next_value()?);
                    }
                }
                "timestamp" => {
                    if timestamp.is_none() {
                        timestamp = Some(access.next_value()?);
                    }
                }
                "value" => {
                    if value.is_none() {
                        let val: Value = access.next_value()?;
                        let measurement_value = if val.is_f64() {
                            WrappedMeasurementValue::F64(val.as_f64().unwrap())
                        } else if val.is_u64() {
                            WrappedMeasurementValue::U64(val.as_u64().unwrap())
                        } else {
                            return Err(de::Error::custom("Unsupported value type for measurement value"));
                        };
                        value = Some(measurement_value);
                    }
                }
                _ => {
                    let _: de::IgnoredAny = access.next_value()?;
                }
            }
        }

        // Ensure all required fields are present
        let device_id = device_id.ok_or_else(|| de::Error::custom("missing field device_id"))?;
        let labels = labels.ok_or_else(|| de::Error::custom("missing field labels"))?;
        let metric_id = metric_id.ok_or_else(|| de::Error::custom("missing field metric_id"))?;
        let timestamp = timestamp.ok_or_else(|| de::Error::custom("missing field timestamp"))?;
        let value = value.ok_or_else(|| de::Error::custom("missing field value"))?;

        Ok(MeasureKwollect {
            device_id,
            labels,
            metric_id,
            timestamp,
            value,
        })
    }
}

/// Implements deserialization for MeasureKwollect which allows JSON data to be converted into a MeasureKwollect instance.
impl<'de> Deserialize<'de> for MeasureKwollect {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(MeasureKwollectVisitor)
    }
}

/// Parses a JSON array of measurements and returns a vector of MeasureKwollect objects.
pub fn parse_measurements(data: Value) -> anyhow::Result<Vec<MeasureKwollect>> {
    log::debug!("Raw data to parse: {data:?}");
    let measurements = data.as_array().context("Expected an array of measurements")?;
    log::debug!("Total measurements in JSON array: {}", measurements.len());
    measurements
        .iter()
        .map(|measurement| {
            log::debug!("Parsing measurement: {measurement:?}");
            serde_json::from_value::<MeasureKwollect>(measurement.clone()).context("Failed to deserialize measurement")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Test for parsing a measurement with power consumption data.
    fn test_parse_measurement_with_power_consumption() {
        let power_consumption_measurement = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": "2025-07-21T16:15:31+02:00",
            "value": 131.7,
            "labels": {
                "_device_orig": "wattmetre1-port6"
            }
        });

        let parsed_measurement = serde_json::from_value::<MeasureKwollect>(power_consumption_measurement);
        assert!(
            parsed_measurement.is_ok(),
            "Failed to parse measurement: {:?}",
            parsed_measurement.err()
        );

        let parsed_measurement = parsed_measurement.unwrap();
        assert_eq!(parsed_measurement.device_id, "taurus-7");
        assert_eq!(parsed_measurement.metric_id, "wattmetre_power_watt");
        assert!(
            matches!(parsed_measurement.value, WrappedMeasurementValue::F64(v) if (v - 131.7).abs() < f64::EPSILON)
        );

        assert!(parsed_measurement.labels.contains_key("_device_orig"));
        assert_eq!(
            parsed_measurement.labels.get("_device_orig"),
            Some(&AttributeValue::String("wattmetre1-port6".to_string()))
        );
    }

    #[test]
    fn test_parse_measurement_with_different_value_types() {
        let measurement_with_u64 = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": "2025-07-21T16:15:31+02:00",
            "value": 131,
            "labels": {
                "_device_orig": "wattmetre1-port6"
            }
        });

        let parsed_measurement = serde_json::from_value::<MeasureKwollect>(measurement_with_u64);
        assert!(
            parsed_measurement.is_ok(),
            "Failed to parse measurement: {:?}",
            parsed_measurement.err()
        );

        let parsed_measurement = parsed_measurement.unwrap();
        assert_eq!(parsed_measurement.device_id, "taurus-7");
        assert_eq!(parsed_measurement.metric_id, "wattmetre_power_watt");
        assert!(matches!(parsed_measurement.value, WrappedMeasurementValue::U64(131)));
    }

    #[test]
    fn test_parse_measurement_with_different_timestamp_format() {
        let measurement_with_string_timestamp = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": "2025-07-21T16:15:31+02:00",
            "value": 131.7,
            "labels": {
                "_device_orig": "wattmetre1-port6"
            }
        });

        let parsed_measurement = serde_json::from_value::<MeasureKwollect>(measurement_with_string_timestamp);
        assert!(
            parsed_measurement.is_ok(),
            "Expected to successfully parse measurement with string timestamp"
        );
    }

    #[test]
    fn test_parse_measurement_with_array_labels() {
        let measurement_with_array_labels = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": "2025-07-21T16:15:31+02:00",
            "value": 131.7,
            "labels": {
                "_device_orig": ["wattmetre1-port6", "wattmetre2-port7"]
            }
        });

        let parsed_measurement = serde_json::from_value::<MeasureKwollect>(measurement_with_array_labels);
        assert!(
            parsed_measurement.is_ok(),
            "Failed to parse measurement: {:?}",
            parsed_measurement.err()
        );

        let parsed_measurement = parsed_measurement.unwrap();
        assert_eq!(parsed_measurement.device_id, "taurus-7");
        assert_eq!(parsed_measurement.metric_id, "wattmetre_power_watt");
        assert!(
            matches!(parsed_measurement.value, WrappedMeasurementValue::F64(v) if (v - 131.7).abs() < f64::EPSILON)
        );
    }

    #[test]
    fn test_manual_deserialization() {
        let json_data = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": "2025-07-22T08:46:26+02:00",
            "value": 3.8189189189189183,
            "labels": {
                "_device_orig": ["wattmetre1-port6"]
            }
        });

        let parsed_measurement = serde_json::from_value::<MeasureKwollect>(json_data);
        assert!(
            parsed_measurement.is_ok(),
            "Failed to parse measurement: {:?}",
            parsed_measurement.err()
        );

        let parsed_measurement = parsed_measurement.unwrap();
        assert_eq!(parsed_measurement.device_id, "taurus-7");
        assert_eq!(parsed_measurement.metric_id, "wattmetre_power_watt");
        assert!(
            matches!(parsed_measurement.value, WrappedMeasurementValue::F64(v) if (v - 3.8189189189189183).abs() < f64::EPSILON)
        );
    }
}
