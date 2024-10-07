use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use alumet::pipeline::{
    elements::output::{builder::AsyncOutputRegistration, BoxedAsyncOutput},
    registry::listener::{MetricListener, MetricListenerRegistration},
};
use alumet::plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    AlumetPluginStart, AlumetPreStart, ConfigTable,
};
use anyhow::Context;

pub struct RelayClientPlugin {
    config: config::Config,
    metric_ids: Arc<Mutex<HashMap<u64, u64>>>,
}

mod config {
    use std::{str::FromStr, time::Duration};

    use serde::{de, Deserialize, Serialize};

    use crate::client::AsciiString;

    #[derive(Serialize, Deserialize)]
    pub struct Config {
        /// The name that this client will use to identify itself to the collector server.
        /// Defaults to the hostname.
        #[serde(default = "default_client_name")]
        pub client_name: AsciiString,

        /// The URI of the collector, for instance `http://127.0.0.1:50051`.
        #[serde(default = "default_collector_uri")]
        pub collector_uri: String,

        /// Maximum number of elements to keep in the output buffer before sending it.
        pub buffer_size: usize,

        /// Maximum amount of time to wait before sending the measurements to the server.
        #[serde(with = "humantime_serde")]
        pub buffer_timeout: Duration,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                client_name: default_client_name(),
                collector_uri: default_collector_uri(),
                buffer_size: 4096,
                buffer_timeout: Duration::from_secs(30),
            }
        }
    }

    fn default_client_name() -> AsciiString {
        let binding = hostname::get()
            .expect("No client_name specified in the config, and unable to retrieve the hostname of the current node.");
        let hostname = binding.to_string_lossy();
        AsciiString::from_str(&hostname).unwrap_or_else(|_| {
            log::error!(
                "I tried to use '{hostname}' as a default client name, but this is not a valid ASCII hostname."
            );
            panic!("hostname {hostname} cannot be used as a client name")
        })
    }

    fn default_collector_uri() -> String {
        String::from("http://[::1]:50051")
    }

    impl<'de> Deserialize<'de> for AsciiString {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            struct V;
            impl<'d> de::Visitor<'d> for V {
                type Value = AsciiString;

                fn expecting(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
                    fmt.write_str("a string containing only visible ASCII characters")
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: de::Error,
                {
                    AsciiString::from_str(v).map_err(|_| E::invalid_value(de::Unexpected::Str(v), &self))
                }
            }
            deserializer.deserialize_str(V)
        }
    }

    impl Serialize for AsciiString {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            self.as_str().serialize(serializer)
        }
    }
}

impl AlumetPlugin for RelayClientPlugin {
    fn name() -> &'static str {
        "relay-client"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        Ok(Some(serialize_config(config::Config::default())?))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        // Read the configuration.
        let config = deserialize_config::<config::Config>(config)?;

        // Initialize a thread-safe HashMap to store the mapping 'local metric id' -> 'collector metric id'
        let metric_ids = Arc::new(Mutex::new(HashMap::new()));

        // Return initialized plugin.
        Ok(Box::new(Self { config, metric_ids }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        let collector_uri = self.config.collector_uri.clone();
        let client_name = self.config.client_name.clone();
        let metric_ids = self.metric_ids.clone();

        let buffer_size = self.config.buffer_size;
        let buffer_timeout = self.config.buffer_timeout;

        // The output is async :)
        alumet.add_async_output_builder(move |ctx, stream| {
            log::info!("Connecting to gRPC server {collector_uri}...");
            // Connect to gRPC server, using the tokio runtime in which Alumet will trigger the output.
            // Note that a Tonic gRPC client can only be used from the runtime it has been initialized with.
            let rt = ctx.async_runtime();
            let client_name_str = client_name.to_string();
            let client = rt
                .block_on(super::grpc::RelayClient::new(collector_uri, client_name, metric_ids))
                .context("gRPC connection error")?;
            log::info!("Successfully connected with client name {client_name_str}.");

            let output = client.process_measurement_stream(stream, buffer_size, buffer_timeout);
            let output: BoxedAsyncOutput = Box::into_pin(Box::new(output));
            Ok(AsyncOutputRegistration {
                name: ctx.output_name("grpc-measurements"),
                output,
            })
        });
        Ok(())
    }

    fn pre_pipeline_start(&mut self, alumet: &mut AlumetPreStart) -> anyhow::Result<()> {
        let collector_uri = self.config.collector_uri.clone();
        let client_name = self.config.client_name.clone();
        let metric_ids = self.metric_ids.clone();

        // Clone the existing metrics (which have been registered by the `start` methods of all the plugins).
        let existing_metrics = alumet.metrics().iter().map(|(id, def)| (*id, def.clone())).collect();

        // Get notified of late metric registration. (TODO: is this the best way? Would it be faster to inspect the points in the output instead?)
        // Also register the existing metrics on the async pipeline.
        alumet.add_metric_listener_builder(move |ctx| {
            let rt = ctx.async_runtime();

            let mut client = rt.block_on(async move {
                // We need to create another client, the one created in `start` has been moved to the output.
                let mut client = super::grpc::RelayClient::new(collector_uri, client_name, metric_ids).await?;

                // Register the existing metrics.
                client.register_metrics(existing_metrics).await?;

                // Pass the client, for use in the listener.
                anyhow::Ok(client)
            })?;

            // Build a listener that uses the client.
            let listener: Box<dyn MetricListener> = Box::new(move |new_metrics| {
                // register the metrics, wait for the message to be sent
                let rt = tokio::runtime::Handle::current();
                rt.block_on(client.register_metrics(new_metrics))?;
                Ok(())
            });

            Ok(MetricListenerRegistration {
                name: ctx.listener_name("grpc-registration"),
                listener,
            })
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
