use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
use serde::{ser::SerializeMap, Serialize};
use std::collections::HashMap;

pub struct Measure {
    pub device_id: String,
    pub labels: HashMap<String, AttributeValue>,
    pub metric_id: String,
    pub timestamp: f64,
    pub value: WrappedMeasurementValue,
}

impl Serialize for Measure {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("_timestamp", &self.timestamp)?;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
    use serde_json::Value;

    use crate::kwollect::Measure;

    #[test]
    fn test_serialize_impl() {
        let entry = Measure {
            device_id: String::from("Iorek"),
            labels: HashMap::new(),
            metric_id: String::from("Byrnison"),
            timestamp: 1750930866.0,
            value: WrappedMeasurementValue::F64(19.0),
        };
        let formated = serde_json::to_value(&entry).unwrap();
        assert!(formated.is_object());
        if let Value::Object(map) = &formated {
            for (key, value) in map {
                match key.as_str() {
                    "_timestamp" => {
                        if let Value::Number(ts) = value {
                            assert_eq!(ts.as_f64().unwrap(), 1750930866.0 as f64)
                        } else {
                            assert!(false)
                        }
                    }
                    "device_id" => {
                        if let Value::String(device_id) = value {
                            assert_eq!(*device_id, String::from("Iorek"))
                        } else {
                            assert!(false)
                        }
                    }
                    "labels" => {
                        assert!(value.is_object())
                    }
                    "metric_id" => {
                        if let Value::String(metric_id) = value {
                            assert_eq!(*metric_id, String::from("Byrnison"))
                        } else {
                            assert!(false)
                        }
                    }
                    "value" => {
                        if let Value::Number(val) = value {
                            assert_eq!(val.as_f64().unwrap(), 19.0 as f64)
                        } else {
                            assert!(false)
                        }
                    }
                    _ => {
                        assert!(false);
                    }
                }
            }
        } else {
            assert!(false)
        }
    }

    #[test]
    fn test_format_to_json_uint() {
        let mut label = HashMap::new();
        label.insert("William".to_string(), AttributeValue::String("Kirjava".to_string()));
        label.insert("Lyra".to_string(), AttributeValue::String("Pantalaimon".to_string()));
        label.insert("Alethiometer".to_string(), AttributeValue::U64(6));
        label.insert("Read".to_string(), AttributeValue::Bool(true));
        label.insert("score".to_string(), AttributeValue::F64(4.4));
        label.insert("author".to_string(), AttributeValue::Str("Philip Pullman"));

        let entry = Measure {
            device_id: String::from("Pantalaimon"),
            labels: label,
            metric_id: String::from("Kirjava"),
            timestamp: 1750930867.0,
            value: WrappedMeasurementValue::U64(12),
        };
        let formated = serde_json::to_value(&entry).unwrap();
        assert!(formated.is_object());
        if let Value::Object(map) = &formated {
            for (key, value) in map {
                match key.as_str() {
                    "_timestamp" => {
                        if let Value::Number(ts) = value {
                            assert_eq!(ts.as_f64().unwrap(), 1750930867.0 as f64)
                        } else {
                            assert!(false)
                        }
                    }
                    "device_id" => {
                        if let Value::String(device_id) = value {
                            assert_eq!(*device_id, String::from("Pantalaimon"))
                        } else {
                            assert!(false)
                        }
                    }
                    "labels" => {
                        assert!(value.is_object());
                        match value {
                            Value::Null => assert!(false),
                            Value::Bool(_) => assert!(false),
                            Value::Number(_) => assert!(false),
                            Value::String(_) => assert!(false),
                            Value::Array(_) => assert!(false),
                            Value::Object(map) => {
                                for (key, value) in map {
                                    match key.as_str() {
                                        "William" => {
                                            assert!(value.is_string());
                                            assert_eq!(value, "Kirjava");
                                        }
                                        "Lyra" => {
                                            assert!(value.is_string());
                                            assert_eq!(value, "Pantalaimon");
                                        }
                                        "Alethiometer" => {
                                            assert!(value.is_u64());
                                            assert_eq!(value, 6);
                                        }
                                        "Read" => {
                                            assert!(value.is_boolean());
                                            assert_eq!(value, true);
                                        }
                                        "score" => {
                                            assert!(value.is_f64());
                                            assert_eq!(value, 4.4 as f64);
                                        }
                                        "author" => {
                                            assert_eq!(value, "Philip Pullman");
                                        }
                                        _ => {
                                            assert!(false);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "metric_id" => {
                        if let Value::String(metric_id) = value {
                            assert_eq!(*metric_id, String::from("Kirjava"))
                        } else {
                            assert!(false)
                        }
                    }
                    "value" => {
                        if let Value::Number(val) = value {
                            assert_eq!(val.as_u64().unwrap(), 12 as u64)
                        } else {
                            assert!(false)
                        }
                    }
                    _ => {
                        assert!(false);
                    }
                }
            }
        } else {
            assert!(false)
        }
    }
}
