//! Kwollect plugin for input is inspired by his neighbor for output, especially this file

use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
use serde::{Serialize, ser::SerializeMap};
use serde_json::Value;
use std::collections::HashMap;

/// A structure to represent a measure collected by Kwollect --> contains each fields of the measure
#[derive(Debug)]
pub struct MeasureKwollect {
    pub device_id: String,
    pub labels: HashMap<String, AttributeValue>,
    pub metric_id: String,
    pub timestamp: f64,
    pub value: WrappedMeasurementValue,
}

// This is the serialize implementation used in Kwollect Plugin for output
/// Serializes a MeasureKwollect instance into a JSON-like map format, including all fields (timestamp, metric_id, device_id, value, and labels).
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

// Here, the main idea is to do contrary of the Kwollect Output Plugin : to deserialize JSON data into MeasureKwollect fields

/// Converts a JSON Value (https://docs.rs/serde_json/latest/serde_json/value/enum.Value.html) into a AttributeValue (https://docs.rs/alumet/latest/alumet/measurement/enum.AttributeValue.html), depending on its type.
/// We need it to be able to use MeasureKwollect with alumet
fn convert_to_attribute_value(value: Value) -> Option<AttributeValue> {
    match value {
        Value::Bool(b) => Some(AttributeValue::Bool(b)),
        Value::Number(n) if n.is_f64() => Some(AttributeValue::F64(n.as_f64().unwrap())),
        Value::Number(n) if n.is_u64() => Some(AttributeValue::U64(n.as_u64().unwrap())),
        Value::String(s) => Some(AttributeValue::String(s)),
        _ => None,
    }
}

/// Parses a JSON array of measurements and returns a vector of MeasureKwollect objects by iterating fn parse_measurements.
pub fn parse_measurements(data: Value) -> Option<Vec<MeasureKwollect>> {
    let measurements = data.as_array()?;
    let mut result = Vec::new();
    for measurement in measurements {
        if let Some(measure) = parse_measurement(measurement) {
            result.push(measure);
        }
    }
    Some(result)
}

/// Parses a single JSON object and converts it into a MeasureKwollect struct, extracting and converting its fields.
fn parse_measurement(measurement: &Value) -> Option<MeasureKwollect> {
    // Conversion to Strings
    let device_id = measurement.get("device_id").and_then(Value::as_str).map(String::from);
    let metric_id = measurement.get("metric_id").and_then(Value::as_str).map(String::from);
    // Conversion to f64
    let timestamp = measurement.get("timestamp").and_then(Value::as_f64);
    // Conversion to a WrappedMeasurementValue (https://docs.rs/alumet/latest/alumet/measurement/enum.WrappedMeasurementValue.html)
    // Similar to a lot of plugins (like csv)
    let value = measurement.get("value").and_then(|v| {
        if v.is_f64() {
            Some(WrappedMeasurementValue::F64(v.as_f64().unwrap()))
        } else if v.is_u64() {
            Some(WrappedMeasurementValue::U64(v.as_u64().unwrap()))
        } else {
            None
        }
    });
    // Conversion to an HashMap
    let labels = measurement.get("labels").and_then(Value::as_object).map(|label_map| {
        label_map
            .iter()
            .filter_map(|(k, v)| convert_to_attribute_value(v.clone()).map(|attr_value| (k.clone(), attr_value)))
            .collect::<HashMap<String, AttributeValue>>()
    });
    // Constructs a MeasureKwollect with each value converted from JSON to a field of it
    Some(MeasureKwollect {
        device_id: device_id?,
        labels: labels?,
        metric_id: metric_id?,
        timestamp: timestamp?,
        value: value?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    #[test]
    fn test_parse_measurement_with_power_consumption() {
        let power_consumption_measurement = serde_json::json!({
            "device_id": "taurus-7",
            "metric_id": "wattmetre_power_watt",
            "timestamp": 1718892920.005984,
            "value": 131.7,
            "labels": {
                "_device_orig": "wattmetre1-port6"
            }
        });

        let parsed_measurement = parse_measurement(&power_consumption_measurement);
        assert!(parsed_measurement.is_some(), "Failed to parse measurement");
        let parsed_measurement = parsed_measurement.unwrap();

        assert_eq!(parsed_measurement.device_id, "taurus-7");
        assert_eq!(parsed_measurement.metric_id, "wattmetre_power_watt");
        assert_eq!(parsed_measurement.value, WrappedMeasurementValue::F64(131.7));

        // Convert timestamp to DateTime and check
        let timestamp = parsed_measurement.timestamp;
        let datetime_utc =
            DateTime::<Utc>::from_timestamp(timestamp as i64, (timestamp.fract() * 1_000_000_000.0) as u32);
        assert!(datetime_utc.is_some());

        assert!(parsed_measurement.labels.contains_key("_device_orig"));
        assert_eq!(
            parsed_measurement.labels.get("_device_orig"),
            Some(&AttributeValue::String("wattmetre1-port6".to_string()))
        );
    }
}
