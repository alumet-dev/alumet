use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, WrappedMeasurementValue},
    pipeline::{
        elements::{error::WriteError, output::OutputContext},
        Output,
    },
    plugin::rust::{deserialize_config, serialize_config, AlumetPlugin},
};

use mongodb::{
    bson::{doc, Document},
    sync::Client,
};
use mongodb2::convert_timestamp;
use serde::{Deserialize, Serialize};

mod mongodb2;

pub struct MongoDbPlugin {
    config: Option<Config>,
}

impl AlumetPlugin for MongoDbPlugin {
    fn name() -> &'static str {
        "mongodb"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let config = deserialize_config(config)?;
        Ok(Box::new(MongoDbPlugin { config: Some(config) }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        let config = self.config.take().unwrap();

        // Test connection to MongoDb.
        let uri = mongodb2::build_mongo_uri(&config);
        let client = Client::with_uri_str(uri)?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        log::info!("Testing connection to MongoDb...");
        rt.block_on(async {
            let collection = client.database(&config.database).collection(&config.collection);
            collection.insert_one(doc! {}).await.unwrap();
        });
        log::info!("Test successful.");

        // Create the output.
        alumet.add_blocking_output(Box::new(MongoDbOutput {
            client: Client::with_uri_str(mongodb2::build_mongo_uri(&config)).unwrap(),
            database: config.database,
            collection: config.collection,
        }));
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct MongoDbOutput {
    client: Client,
    database: String,
    collection: String,
}

impl Output for MongoDbOutput {
    fn write(&mut self, measurements: &MeasurementBuffer, ctx: &OutputContext) -> Result<(), WriteError> {
        // Cannot write anything with an empty buffer.
        if measurements.is_empty() {
            log::warn!("MongoDb received an empty MeasurementBuffer");
            return Ok(());
        }

        // Build the data to send to MongoDb
        let mut docs = Vec::with_capacity(measurements.len());
        for m in measurements {
            let mut doc = Document::new();
            let metric = ctx.metrics.by_id(&m.metric).unwrap();
            doc.insert("measurement", &metric.name);

            // Resources and consumers are translated as fields
            doc.insert("resource_kind", m.resource.kind());
            doc.insert("resource_id", m.resource.id_string().unwrap_or_default());
            doc.insert("resource_consumer_kind", m.consumer.kind());
            doc.insert("resource_consumer_id", m.consumer.id_string().unwrap_or_default());

            // Alumet attributes are translated to fields
            // Some field keys are reserved by Alumet and will trigger a renaming
            let attributes = m.attributes();
            const RESERVED_FIELDS: [&str; 6] = [
                "measurement",
                "resource_kind",
                "resource_id",
                "resource_consumer_kind",
                "resource_consumer_id",
                "value",
            ];

            for (field_key, field_value) in attributes {
                let field_key = if RESERVED_FIELDS.contains(&field_key) {
                    format!("{}_field", field_key)
                } else {
                    String::from(field_key)
                };
                match field_value {
                    AttributeValue::F64(v) => {
                        doc.insert(field_key, v.to_string());
                    }
                    AttributeValue::U64(v) => {
                        doc.insert(field_key, format!("{v}u"));
                    }
                    AttributeValue::Bool(v) => {
                        doc.insert(field_key, if *v { "T" } else { "F" });
                    }
                    AttributeValue::Str(v) => {
                        doc.insert(field_key, v);
                    }
                    AttributeValue::String(v) => {
                        doc.insert(field_key, v);
                    }
                }
            }

            // Append alumet value
            match m.value {
                WrappedMeasurementValue::F64(v) => {
                    doc.insert("value", v.to_string());
                }
                WrappedMeasurementValue::U64(v) => {
                    doc.insert("value", format!("{v}u"));
                }
            }

            // Add the timestamp
            doc.insert("timestamp", convert_timestamp(m.timestamp));

            docs.push(doc);
        }

        // Send the data to MongoDb
        let collection = self.client.database(&self.database).collection(&self.collection);
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            collection.insert_many(docs).await.unwrap();
        });

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    host: String,
    port: String,
    database: String,
    collection: String,
    username: Option<String>,
    password: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::from("localhost"),
            port: String::from("27017"),
            database: String::from("FILL ME"),
            collection: String::from("FILL ME"),
            username: Some(String::from("FILL ME")),
            password: Some(String::from("FILL ME")),
        }
    }
}
