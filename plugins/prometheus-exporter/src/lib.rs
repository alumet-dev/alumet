mod output;

use alumet::plugin::rust::{AlumetPlugin, deserialize_config, serialize_config};
use hyper::http::StatusCode;
use hyper::{
    Body, Request, Response, Server,
    service::{make_service_fn, service_fn},
};
use output::PrometheusOutput;
use prometheus_client::encoding::text::encode;
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder;
use tokio::sync::oneshot;

pub struct PrometheusPlugin {
    config: Config,
    shutdown_tx_server: Option<oneshot::Sender<()>>,
}

impl AlumetPlugin for PrometheusPlugin {
    fn name() -> &'static str {
        "prometheus-exporter"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<alumet::plugin::ConfigTable>> {
        Ok(Some(serialize_config(Config::default())?))
    }

    fn init(config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        let plugin_config = deserialize_config(config)?;
        Ok(Box::new(PrometheusPlugin {
            config: plugin_config,
            shutdown_tx_server: None,
        }))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetPluginStart) -> anyhow::Result<()> {
        // Create a new PrometheusOutput instance
        let output = Box::new(PrometheusOutput::new(
            self.config.add_attributes_to_labels,
            self.config.port,
            self.config.host.clone(),
            self.config.prefix.clone(),
            self.config.suffix.clone(),
        )?);

        // Create shutdown channel to close the server thread
        let (shutdown_tx_server, shutdown_rx_server) = oneshot::channel::<()>();

        // Clone the state to pass it down the coroutine
        let output_clone = output.clone();
        std::thread::spawn(move || {
            let rt = Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create Tokio runtime for the prometheus exporter");

            // Execute the server inside the current thread runtime
            rt.block_on(async {
                let state_clone = output_clone.state.clone();
                let addr_clone = output_clone.addr;
                let make_svc = make_service_fn(move |_conn| {
                    let state = state_clone.clone();
                    async move {
                        Ok::<_, hyper::Error>(service_fn(move |req: Request<Body>| {
                            let state = state.clone();
                            async move {
                                if req.uri().path() != "/metrics" {
                                    return Ok::<Response<Body>, hyper::Error>(
                                        Response::builder()
                                            .status(StatusCode::NOT_FOUND)
                                            .body(Body::from("Not Found"))
                                            .unwrap(),
                                    );
                                }
                                let mut buf = String::new();
                                if let Err(e) = encode(&mut buf, &*state.registry.read().await) {
                                    log::error!("Failed to encode metrics: {}", e);
                                    return Ok(Response::builder()
                                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                                        .body(Body::from("Internal Server Error"))
                                        .unwrap());
                                }
                                Ok(Response::builder()
                                    .header(
                                        "Content-Type",
                                        "application/openmetrics-text; version=1.0.0; charset=utf-8",
                                    )
                                    .body(Body::from(buf))
                                    .unwrap())
                            }
                        }))
                    }
                });

                // Start the hyper server
                let server = Server::bind(&addr_clone).serve(make_svc);
                log::info!("Prometheus metrics exporter available on http://{}/metrics", addr_clone);

                // Add graceful shutdown signal
                let graceful = server.with_graceful_shutdown(async {
                    shutdown_rx_server.await.ok();
                });

                // Run the server with graceful shutdown
                if let Err(e) = graceful.await {
                    log::error!("Prometheus server error: {}", e);
                }

                log::info!("Prometheus server stopped gracefully.");
            });
        });

        // Store the shutdown tx handle for later shutdown
        self.shutdown_tx_server = Some(shutdown_tx_server);

        // Add output for processing measurements
        alumet.add_blocking_output("out", output)?;

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Shutting down the Prometheus plugin...");
        if let Some(tx) = self.shutdown_tx_server.take() {
            // Send the shutdown signal to the server thread
            match tx.send(()) {
                Ok(_) => {
                    log::info!("Shutdown signal sent to prometheus server thread.");
                }
                Err(e) => {
                    log::error!(
                        "Failed to send shutdown signal to server thread. Receiver may have already been dropped. {:?}",
                        e
                    );
                }
            }
        }

        log::info!("Prometheus plugin shutdown complete.");

        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    host: String,
    prefix: String,
    suffix: String,
    port: u16,
    add_attributes_to_labels: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: String::from("0.0.0.0"),
            prefix: String::from(""),
            suffix: String::from("_alumet"),
            port: 9091,
            add_attributes_to_labels: true,
        }
    }
}
