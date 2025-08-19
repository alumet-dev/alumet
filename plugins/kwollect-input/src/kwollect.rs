// This module provides functionality to serialize and deserialize measurement data for Kwollect.

use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
use anyhow::Context;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// A structure to represent a measure collected by Kwollect.
#[derive(Debug, Serialize, Deserialize)]
pub struct MeasureKwollect {
    pub device_id: String,
    #[serde(with = "labels")]
    pub labels: HashMap<String, AttributeValue>,
    pub metric_id: String,
    pub timestamp: String,
    #[serde(with = "value")]
    pub value: WrappedMeasurementValue,
}

// MeasurementValue is not define (de)serializable by serde so we need to do a wrapper
/// Serializable wrappers for measurement value
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerializableMeasurementValue {
    U64(u64),
    F64(f64),
}

impl From<WrappedMeasurementValue> for SerializableMeasurementValue {
    fn from(value: WrappedMeasurementValue) -> Self {
        match value {
            WrappedMeasurementValue::U64(v) => Self::U64(v),
            WrappedMeasurementValue::F64(v) => Self::F64(v),
        }
    }
}

impl From<SerializableMeasurementValue> for WrappedMeasurementValue {
    fn from(value: SerializableMeasurementValue) -> Self {
        match value {
            SerializableMeasurementValue::U64(v) => Self::U64(v),
            SerializableMeasurementValue::F64(v) => Self::F64(v),
        }
    }
}

// We needed (de)serializer to implement Value for MeasureKwollect so we used a module to simplify
/// Serde helper for WrappedMeasurementValue
mod value {
    use super::*;

    pub fn serialize<S>(value: &WrappedMeasurementValue, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        SerializableMeasurementValue::from(value.clone()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<WrappedMeasurementValue, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(SerializableMeasurementValue::deserialize(deserializer)?.into())
    }
}

// We needed (de)serializer to implement Label for MeasureKwollect so we used a module to simplify
/// Serde helper to convert HashMap<String, AttributeValue> to & from JSON.
mod labels {
    use super::*;

    pub fn serialize<S>(value: &HashMap<String, AttributeValue>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let map: Map<String, Value> = value
            .iter()
            .map(|(k, v)| {
                let json_val = match v {
                    AttributeValue::Bool(b) => Value::Bool(*b),
                    AttributeValue::F64(f) => {
                        Value::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| serde_json::Number::from(0)))
                    }
                    AttributeValue::U64(u) => Value::Number(serde_json::Number::from(*u)),
                    AttributeValue::Str(s) => Value::String(s.to_string()),
                    AttributeValue::String(s) => Value::String(s.clone()),
                    AttributeValue::ListU64(list) => {
                        let list_as_vec: Vec<Value> = list
                            .iter()
                            .map(|u| Value::Number(serde_json::Number::from(*u)))
                            .collect();
                        Value::Array(list_as_vec)
                    }
                };
                (k.clone(), json_val)
            })
            .collect();
        map.serialize(serializer)
    }

    // labels is a HashMap<String, AttributeValue>: deserialize for each type of AttributeValue
    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, AttributeValue>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let label_map: Map<String, Value> = Map::deserialize(deserializer)?;
        let mut labels_map = HashMap::with_capacity(label_map.len());

        for (k, v) in label_map {
            let attribute_value = match v {
                Value::Bool(b) => AttributeValue::Bool(b),
                Value::Number(n) if n.is_f64() => AttributeValue::F64(n.as_f64().unwrap()),
                Value::Number(n) if n.is_u64() => AttributeValue::U64(n.as_u64().unwrap()),
                Value::Number(n) if n.is_i64() => AttributeValue::U64(n.as_i64().unwrap() as u64),
                Value::String(s) => AttributeValue::String(s),
                Value::Array(arr) => {
                    let array_as_string = arr.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", ");
                    AttributeValue::String(array_as_string)
                }
                _ => AttributeValue::String(v.to_string()),
            };
            labels_map.insert(k, attribute_value);
        }
        Ok(labels_map)
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
