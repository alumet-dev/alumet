use alumet::metrics::{Metric, RawMetricId};
use alumet::pipeline::elements::output::{builder::AsyncOutputRegistration, BoxedAsyncOutput};
use alumet::plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    AlumetPluginStart, ConfigTable,
};
use anyhow::Context;
use tokio::sync::mpsc;

use crate::client::output;

use super::retry::ExponentialRetryPolicy;

pub struct RelayClientPlugin {
    config: Option<config::Config>,
}

mod config {
    use std::time::Duration;

    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Config {
        /// The name that this client will use to identify itself to the collector server.
        /// Defaults to the hostname.
        #[serde(default = "default_client_name")]
        pub client_name: String,

        /// The host and port of the collector, for instance `127.0.0.1:50051`.
        #[serde(default = "default_relay_server_address")]
        pub relay_server: String,

        /// Maximum number of elements to keep in the output buffer before sending it.
        pub buffer_max_length: usize,

        /// Maximum amount of time to wait before sending the measurements to the server.
        #[serde(with = "humantime_serde")]
        pub buffer_timeout: Duration,

        /// Parameter of the exponential backoff strategy that is applied when a network operation fails.
        ///
        /// The delay is multiplied by two after each attempt.
        pub retry: RetryConfig,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct RetryConfig {
        /// Maximum number of retries before giving up.
        pub max_times: u16,

        /// Initial delay between two attempts.
        #[serde(with = "humantime_serde")]
        pub initial_delay: Duration,

        /// Maximum delay between two attempts.
        #[serde(with = "humantime_serde")]
        pub max_delay: Duration,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                client_name: default_client_name(),
                relay_server: default_relay_server_address(),
                buffer_max_length: 4096,
                buffer_timeout: Duration::from_secs(30),
                retry: RetryConfig::default(),
            }
        }
    }

    impl Default for RetryConfig {
        fn default() -> Self {
            Self {
                max_times: 8,
                initial_delay: Duration::from_millis(500),
                max_delay: Duration::from_secs(4),
            }
        }
    }

    fn default_client_name() -> String {
        let binding = hostname::get()
            .expect("No client_name specified in the config, and unable to retrieve the hostname of the current node.");
        binding.to_string_lossy().to_string()
    }

    fn default_relay_server_address() -> String {
        String::from("[::1]:50051")
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

        // Return initialized plugin.
        Ok(Box::new(Self { config: Some(config) }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // Prepare the values that will be moved to the closure.
        let config = self.config.take().unwrap();
        let client_settings = output::Settings {
            client_name: config.client_name,
            server_address: config.relay_server,
            buffer: output::BufferSettings {
                initial_capacity: 512,
                max_length: config.buffer_max_length,
                timeout: config.buffer_timeout,
            },
            msg_retry: ExponentialRetryPolicy {
                max_retrys: config.retry.max_times,
                initial_delay: config.retry.initial_delay,
                max_delay: config.retry.max_delay,
                multiplier: 2,
            },
            init_retry: ExponentialRetryPolicy {
                max_retrys: config.retry.max_times,
                initial_delay: config.retry.initial_delay,
                max_delay: config.retry.max_delay,
                multiplier: 2,
            },
        };

        // Create a channel for the metrics.
        // We want only one task to use the TcpOutput, otherwise it would cause interleaving writes and mess up the messages we send.
        let (metrics_tx, metrics_rx) = mpsc::unbounded_channel();

        // The output is async :)
        alumet.add_async_output_builder(move |ctx, stream| {
            let alumet_link = output::AlumetLink {
                in_measurements: stream,
                in_metrics: metrics_rx,
                metrics_reader: ctx.metrics_reader(),
            };

            let tcp = ctx
                .async_runtime()
                .block_on(super::output::TcpOutput::connect(alumet_link, client_settings))
                .context("relay connection error")?;

            let output: BoxedAsyncOutput = Box::pin(tcp.send_loop());
            Ok(AsyncOutputRegistration {
                name: ctx.output_name("relay-tcp"),
                output,
            })
        });

        alumet.on_pre_pipeline_start(move |pre_start| {
            // register the existing metrics
            let existing_metrics: Vec<(RawMetricId, Metric)> =
                pre_start.metrics().iter().map(|(id, def)| (*id, def.clone())).collect();
            metrics_tx
                .send(existing_metrics)
                .context("failed to send the initial metrics to the TCP output")?;

            // hook to register the late metrics
            pre_start.add_metric_listener(move |new_metrics| {
                metrics_tx
                    .send(new_metrics)
                    .context("failed to send late metrics to the TCP output")
            });
            Ok(())
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
