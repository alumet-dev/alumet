use std::collections::HashSet;

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, WrappedMeasurementValue},
    pipeline::{
        Output,
        elements::{
            error::WriteError,
            output::{OutputContext, error::WriteRetry},
        },
    },
    plugin::rust::{AlumetPlugin, deserialize_config, serialize_config},
};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::influxdb2::LineProtocolData;

mod influxdb2;

pub struct InfluxDbPlugin {
    config: Option<Config>,
}

impl AlumetPlugin for InfluxDbPlugin {
    fn name() -> &'static str {
        "influxdb"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(InfluxDbPlugin { config: Some(config) }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let config = self.config.take().unwrap();

        // Connect to InfluxDB to detect configuration errors early.
        let influx_client = influxdb2::Client::new(config.host.clone(), config.token.clone());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        log::info!("Testing connection to InfluxDB...");
        rt.block_on(influx_client.test_write(&config.org, &config.bucket))
            .with_context(|| {
                format!(
                    "Cannot write to InfluxDB host {} in org {} and bucket {}. Please check your configuration.",
                    &config.host, &config.org, &config.bucket
                )
            })?;
        log::info!("Test successful.");

        // Create the output.
        alumet.add_blocking_output(
            "out",
            Box::new(InfluxDbOutput {
                client: influx_client,
                org: config.org,
                bucket: config.bucket,
                attributes_as: config.attributes_as,
                attributes_as_tags: config.attributes_as_tags.unwrap_or_default(),
                attributes_as_fields: config.attributes_as_fields.unwrap_or_default(),
            }),
        )?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct InfluxDbOutput {
    client: influxdb2::Client,
    org: String,
    bucket: String,
    attributes_as: AttributeAs,
    attributes_as_tags: HashSet<String>,
    attributes_as_fields: HashSet<String>,
}

impl Output for InfluxDbOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        // Cannot write anything with an empty buffer.
        if measurements.is_empty() {
            log::warn!("InfluxDb received an empty MeasurementBuffer");
            return Ok(());
        }

        // Build the data to send to InfluxDB.
        let mut builder = LineProtocolData::builder();
        for m in measurements {
            let metric = ctx.metrics.by_id(&m.metric).unwrap();
            builder.measurement(&metric.name);

            // Resources and consumers are translated to tags.
            builder.tag("resource_kind", m.resource.kind());
            builder.tag("resource_id", &m.resource.id_string().unwrap_or_default());
            builder.tag("resource_consumer_kind", m.consumer.kind());
            builder.tag("resource_consumer_id", &m.consumer.id_string().unwrap_or_default());

            // Alumet attributes are translated to fields, or tags, depending on the configuration.
            // Some tag keys and field keys are reserved by Alumet and will trigger a renaming.
            const RESERVED_TAGS: [&str; 4] = [
                "resource_kind",
                "resource_id",
                "resource_consumer_kind",
                "resource_consumer_id",
            ];
            const RESERVED_FIELD: &str = "value";

            let (tags, fields): (Vec<_>, Vec<_>) = m.attributes().partition(|(key, _)| {
                partition_tag(
                    key,
                    self.attributes_as,
                    &self.attributes_as_tags,
                    &self.attributes_as_fields,
                )
            });

            // Append tags.
            for (tag_key, tag_value) in tags {
                builder.tag(
                    &ensure_valid_tag_key(RESERVED_TAGS, tag_key.to_string()),
                    &tag_value.to_string(),
                );
            }
            // Append fields.
            for (field_key, field_value) in fields {
                let field_key = ensure_valid_field_key(RESERVED_FIELD, field_key);
                match field_value {
                    AttributeValue::F64(v) => builder.field_float(field_key, *v),
                    AttributeValue::U64(v) => builder.field_uint(field_key, *v),
                    AttributeValue::Bool(v) => builder.field_bool(field_key, *v),
                    AttributeValue::Str(v) => builder.field_string(field_key, v),
                    AttributeValue::String(v) => builder.field_string(field_key, v),
                    AttributeValue::ListU64(items) => {
                        builder.field_string(field_key, &itertools::join(items.iter(), ","))
                    }
                };
            }

            // Alumet value is a field.
            match m.value {
                WrappedMeasurementValue::F64(v) => builder.field_float("value", v),
                WrappedMeasurementValue::U64(v) => builder.field_uint("value", v),
            };

            // And the timestamp comes last.
            builder.timestamp(m.timestamp);
        }
        let data = builder.build();
        log::debug!("Line protocol data: {data:?}");

        // Do the writing on the tokio Runtime.
        let handle = tokio::runtime::Handle::current();
        handle
            .block_on(self.client.write(&self.org, &self.bucket, data))
            .context("failed to write measurements to InfluxDB")
            .retry_write()?;
        Ok(())
    }
}

// Returns true if the attribute with this key should be serialized as an InfluxDB tag,
// false if it should become a field.
fn partition_tag(
    key: &str,
    attributes_as: AttributeAs,
    attributes_as_tags: &HashSet<String>,
    attributes_as_fields: &HashSet<String>,
) -> bool {
    match attributes_as {
        AttributeAs::Tag => {
            // default is tag => tag unless if in set
            !attributes_as_fields.contains(key)
        }
        AttributeAs::Field => {
            // default is field => tag only if in set
            attributes_as_tags.contains(key)
        }
    }
}

// Check if the tag key is reserved and in case it is append a prefix
fn ensure_valid_tag_key(reserved_tags: [&str; 4], tag_key: String) -> String {
    if reserved_tags.contains(&tag_key.as_str()) {
        format!("alumet_attribute__{tag_key}")
    } else {
        tag_key
    }
}

// Check if the field key is reserved and in case it is return a predefined key
fn ensure_valid_field_key<'a>(reserved_field: &'a str, field_key: &'a str) -> &'a str {
    if field_key == reserved_field {
        "alumet_attribute__value"
    } else {
        field_key
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address of the host where InfluxDB is running
    pub host: String,
    /// Token to write on the database
    pub token: String,
    /// Organisation where to write data
    pub org: String,
    /// Bucket where to write data   
    pub bucket: String,
    /// By default, serialize all Alumet attributes as fields. This can be either `"field"` or `"tag".
    pub attributes_as: AttributeAs,
    /// Always serialize the given list of attributes as InfluxDB tags
    pub attributes_as_tags: Option<HashSet<String>>,
    /// Always serialize the given list of attributes as InfluxDB fields
    pub attributes_as_fields: Option<HashSet<String>>,
}

/// How to serialize Alumet attributes by default?
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum AttributeAs {
    /// Serialize attributes as InfluxDB tags, except if their key
    /// is in `attributes_as_fields`.
    Tag,
    /// Serialize attributes as InfluxDB fields, except if their key
    /// is in `attributes_as_tags`.
    Field,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::from("http://localhost:8086"),
            token: String::from("FILL ME"),
            org: String::from("FILL ME"),
            bucket: String::from("FILL ME"),
            attributes_as: AttributeAs::Field,
            attributes_as_tags: None,
            attributes_as_fields: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{AttributeAs, ensure_valid_field_key, ensure_valid_tag_key, partition_tag};
    use std::collections::HashSet;
    #[test]
    fn test_partition_tag() {
        let mut tags: HashSet<String> = HashSet::new();
        tags.insert("is_a_tag".to_string());
        tags.insert("is_another_tag".to_string());

        let mut fields: HashSet<String> = HashSet::new();
        fields.insert("is_a_field".to_string());
        fields.insert("is_another_field".to_string());

        assert_eq!(partition_tag("is_a_tag", AttributeAs::Tag, &tags, &fields), true);
        assert_eq!(partition_tag("is_a_field", AttributeAs::Tag, &tags, &fields), false);
        assert_eq!(partition_tag("is_nothing", AttributeAs::Tag, &tags, &fields), true);
        assert_eq!(partition_tag("is_a_tag", AttributeAs::Field, &tags, &fields), true);
        assert_eq!(partition_tag("is_a_field", AttributeAs::Field, &tags, &fields), false);
        assert_eq!(partition_tag("is_nothing", AttributeAs::Field, &tags, &fields), false);
    }

    #[test]
    fn test_tag_keys() {
        let reserved_tags: [&str; 4] = [
            "resource_kind",
            "resource_id",
            "resource_consumer_kind",
            "resource_consumer_id",
        ];

        assert_eq!(
            ensure_valid_tag_key(reserved_tags, "resource_kind".to_string()),
            "alumet_attribute__resource_kind"
        );
        assert_eq!(
            ensure_valid_tag_key(reserved_tags, "some_random_tag".to_string()),
            "some_random_tag"
        );
    }

    #[test]
    fn test_field_keys() {
        let reserved_field = "value";

        assert_eq!(
            ensure_valid_field_key(reserved_field, "value"),
            "alumet_attribute__value"
        );
        assert_eq!(
            ensure_valid_field_key(reserved_field, "some_random_field"),
            "some_random_field"
        );
    }
}
