//! Kwollect plugin for input is inspired by its neighbor for output, especially this file because of Serialize implementation.
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
    pub timestamp: f64,
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
                // labels is an HashMap<String, AttributeValue>  so we need to deserialize for each type of AttributeValue
                "labels" => {
                    if labels.is_none() {
                        let label_map: Map<String, Value> = access.next_value()?;
                        let mut labels_map = HashMap::new();
                        for (k, v) in label_map {
                            let attribute_value = match v {
                                Value::Bool(b) => AttributeValue::Bool(b),
                                Value::Number(n) if n.is_f64() => AttributeValue::F64(n.as_f64().unwrap()),
                                Value::Number(n) if n.is_u64() => AttributeValue::U64(n.as_u64().unwrap()),
                                Value::String(s) => AttributeValue::String(s),
                                _ => return Err(de::Error::custom("Unsupported value type")),
                            };
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
    let measurements = data.as_array().context("Expected an array of measurements")?;
    measurements
        .iter()
        .map(|measurement| {
            serde_json::from_value::<MeasureKwollect>(measurement.clone()).context("Failed to deserialize measurement")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    #[test]
    /// Test for parsing a measurement with power consumption data.
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
