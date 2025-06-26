use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
use serde_json::Value;
use std::collections::HashMap;

pub struct Measure {
    pub device_id: String,
    pub label: HashMap<String, AttributeValue>,
    pub name: String,
    pub timestamp: f64,
    pub value: WrappedMeasurementValue,
}

pub fn format_to_json(entry: Measure) -> Value {
    let mut obj = serde_json::map::Map::new();
    obj.insert(
        "_timestamp".to_string(),
        Value::Number(serde_json::Number::from_f64(entry.timestamp).unwrap()),
    );
    obj.insert("metric_id".to_string(), Value::String(entry.name));
    match entry.value {
        WrappedMeasurementValue::F64(value) => {
            obj.insert(
                "value".to_string(),
                Value::Number(serde_json::Number::from_f64(value).unwrap()),
            );
        }
        WrappedMeasurementValue::U64(value) => {
            obj.insert(
                "value".to_string(),
                Value::Number(serde_json::Number::from_u128(value as u128).unwrap()),
            );
        }
    }
    obj.insert("device_id".to_string(), Value::String(entry.device_id));
    let mut labels = serde_json::map::Map::new();
    for attr in entry.label {
        labels.insert(attr.0.to_string(), Value::String(attr.1.to_string()));
    }
    obj.insert("labels".to_string(), Value::Object(labels));

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alumet::measurement::{AttributeValue, WrappedMeasurementValue};
    use serde_json::Value;

    use crate::kwollect::{format_to_json, Measure};

    #[test]
    fn test_format_to_json_f64() {
        let entry = Measure {
            device_id: String::from("Iorek"),
            label: HashMap::new(),
            name: String::from("Byrnison"),
            timestamp: 1750930866.0,
            value: WrappedMeasurementValue::F64(19.0),
        };
        let formated = format_to_json(entry);
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
                        if let Value::String(device) = value {
                            assert_eq!(*device, String::from("Iorek"))
                        } else {
                            assert!(false)
                        }
                    }
                    "labels" => {
                        assert!(value.is_object())
                    }
                    "metric_id" => {
                        if let Value::String(device) = value {
                            assert_eq!(*device, String::from("Byrnison"))
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

        let entry = Measure {
            device_id: String::from("Pantalaimon"),
            label: label,
            name: String::from("Kirjava"),
            timestamp: 1750930867.0,
            value: WrappedMeasurementValue::U64(12),
        };
        let formated = format_to_json(entry);
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
                        if let Value::String(device) = value {
                            assert_eq!(*device, String::from("Pantalaimon"))
                        } else {
                            assert!(false)
                        }
                    }
                    "labels" => {
                        assert!(value.is_object())
                    }
                    "metric_id" => {
                        if let Value::String(device) = value {
                            assert_eq!(*device, String::from("Kirjava"))
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
