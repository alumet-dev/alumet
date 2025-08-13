//! Implementation of a small subset of the REST API of OpenSearch/ElasticSearch.

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, WrappedMeasurementValue},
    pipeline::elements::output::OutputContext,
};

use serde::{Serialize, ser::Error, ser::SerializeMap};
use serde_json::json;
use std::{collections::HashMap, time::SystemTime};
use time::{UtcDateTime, format_description::well_known::Rfc3339};

/// OpenSearch/ElasticSearch serializer helper.
pub struct Serializer {
    /// Each index will be named like `"{index_prefix}-{metric_name}"`
    pub index_prefix: String,

    pub metric_unit_as_index_suffix: bool,
}

impl Serializer {
    /// Generates the mappings for an index.
    pub fn common_index_mappings(&self) -> serde_json::Value {
        json!({
            "properties": DocMeasurement::properties_definitions()
        })
    }

    /// Generates the body of a bulk document creation request.
    ///
    /// Each measurement point is created as a separate document with a `@timestamp` field.
    /// The remaining fields depend on the client settings.
    pub fn body_bulk_create_docs(
        &self,
        measurement_points: &MeasurementBuffer,
        ctx: &OutputContext,
    ) -> std::io::Result<String> {
        // The bulk body is made of (action, document) pairs.
        // Each element is serialized as a json object and separated by a newline at the end.
        let mut bytes = Vec::new();
        for m in measurement_points {
            // TODO provide m.metric.fetch_definition(ctx) ?

            self.serialize_bulk_action(&mut bytes, m, ctx)?;
            bytes.push(b'\n');

            self.serialize_bulk_document(&mut bytes, m, ctx)?;
            bytes.push(b'\n');
        }

        // SAFETY: serde_json outputs valid utf8 chars
        Ok(unsafe { String::from_utf8_unchecked(bytes) })
    }

    fn serialize_bulk_action(
        &self,
        buf: &mut Vec<u8>,
        m: &MeasurementPoint,
        ctx: &OutputContext,
    ) -> serde_json::Result<()> {
        let index = {
            // {prefix}-{metric} or {prefix}-{metric}-{suffix}
            let metric = ctx.metrics.by_id(&m.metric).unwrap();
            let metric_name = metric.name.to_owned();
            let index_prefix = &self.index_prefix;
            let mut buf = String::from(index_prefix);
            buf.push('-');
            buf.push_str(&metric_name);
            if self.metric_unit_as_index_suffix {
                let index_suffix = metric.unit.unique_name();
                buf.push('-');
                buf.push_str(&index_suffix);
            };
            buf
        };
        let action = BulkAction::Create { index };
        serde_json::to_writer(buf, &action)
    }

    fn serialize_bulk_document(
        &self,
        buf: &mut Vec<u8>,
        measurement: &MeasurementPoint,
        _ctx: &OutputContext,
    ) -> serde_json::Result<()> {
        let doc = DocMeasurement { measurement };
        serde_json::to_writer(buf, &doc)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum BulkAction {
    Create {
        #[serde(rename = "_index")]
        index: String,
    },
}

/// A structure that allows a custom serialization of MeasurementPoints.
struct DocMeasurement<'a> {
    measurement: &'a MeasurementPoint,
}

impl Serialize for DocMeasurement<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(Some(6))?;
        // timestamp
        let datetime = UtcDateTime::from(SystemTime::from(self.measurement.timestamp));
        let datetime = datetime.format(&Rfc3339).map_err(S::Error::custom)?;
        map.serialize_entry("@timestamp", &datetime)?;

        // resource and consumer
        map.serialize_entry("resource_kind", self.measurement.resource.kind())?;
        // TODO there should be a nicer way to get a &str from a resource id without allocating a string
        map.serialize_entry("resource_id", &self.measurement.resource.id_display().to_string())?;
        map.serialize_entry("consumer_kind", &self.measurement.consumer.kind())?;
        map.serialize_entry("consumer_id", &self.measurement.consumer.id_display().to_string())?;

        // value
        match self.measurement.value {
            WrappedMeasurementValue::F64(v) => map.serialize_entry("value", &v)?,
            WrappedMeasurementValue::U64(v) => map.serialize_entry("value", &v)?,
        };

        // attributes
        const RESERVED_KEYS: [&str; 5] = [
            "@timestamp",
            "resource_kind",
            "resource_id",
            "consumer_kind",
            "consumer_id",
        ];
        for (key, attr) in self.measurement.attributes() {
            let key = if RESERVED_KEYS.contains(&key) {
                &format!("__{key}")
            } else {
                key
            };
            match attr {
                AttributeValue::F64(v) => map.serialize_entry(key, v)?,
                AttributeValue::U64(v) => map.serialize_entry(key, v)?,
                AttributeValue::Bool(v) => map.serialize_entry(key, v)?,
                AttributeValue::Str(v) => map.serialize_entry(key, v)?,
                AttributeValue::String(v) => map.serialize_entry(key, v)?,
                AttributeValue::ListU64(v) => map.serialize_entry(key, v)?,
            };
        }

        map.end()
    }
}

impl<'a> DocMeasurement<'a> {
    /// Generates the mappings for an index.
    pub fn properties_definitions() -> serde_json::Map<String, serde_json::Value> {
        fn field_with_type(field: &str, data_type: &str) -> (String, serde_json::Value) {
            (
                field.to_string(),
                json!({
                    "type": data_type
                }),
            )
        }

        serde_json::Map::from_iter([
            field_with_type("@timestamp", "date_nanos"),
            field_with_type("resource_kind", "keyword"),
            field_with_type("resource_id", "keyword"),
            field_with_type("consumer_kind", "keyword"),
            field_with_type("consumer_id", "keyword"),
        ])
    }
}

#[derive(Debug, Serialize)]
pub struct CreateIndexTemplate {
    pub index_patterns: Vec<String>,
    pub template: IndexTemplate,
    pub priority: u32,
    pub version: u32,
    #[serde(rename = "_meta")]
    pub meta: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct IndexTemplate {
    pub mappings: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use alumet::{
        measurement::Timestamp,
        metrics::RawMetricId,
        resources::{Resource, ResourceConsumer},
    };
    use time::OffsetDateTime;

    use super::*;

    /// Parses an RFC 3339 date-and-time string into a Timestamp value.
    pub(crate) fn timestamp_from_rfc3339(timestamp: &str) -> Timestamp {
        SystemTime::from(OffsetDateTime::parse(timestamp, &Rfc3339).unwrap()).into()
    }

    #[test]
    fn test_serialize() {
        let doc = DocMeasurement {
            measurement: &MeasurementPoint::new_untyped(
                timestamp_from_rfc3339("2025-06-06T12:03:18Z"),
                RawMetricId::from_u64(1),
                Resource::LocalMachine,
                ResourceConsumer::LocalMachine,
                WrappedMeasurementValue::U64(64),
            )
            .with_attr_vec(vec![
                ("bool", AttributeValue::Bool(true)),
                ("f64", AttributeValue::F64(0.45)),
                ("u64", AttributeValue::U64(69)),
                ("str", AttributeValue::Str("alumet")),
                ("string", AttributeValue::String("elastic search".to_string())),
            ]),
        };

        let result = serde_json::to_string_pretty(&doc).unwrap();
        let expected_result = "{
  \"@timestamp\": \"2025-06-06T12:03:18Z\",
  \"resource_kind\": \"local_machine\",
  \"resource_id\": \"\",
  \"consumer_kind\": \"local_machine\",
  \"consumer_id\": \"\",
  \"value\": 64,
  \"bool\": true,
  \"f64\": 0.45,
  \"u64\": 69,
  \"str\": \"alumet\",
  \"string\": \"elastic search\"
}";
        assert_eq!(result, expected_result)
    }
}
