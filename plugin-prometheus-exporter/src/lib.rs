use std::net::SocketAddr;

use alumet::plugin::rust::AlumetPlugin;

mod output;

pub struct PrometheusPlugin {}

impl AlumetPlugin for PrometheusPlugin {
    fn name() -> &'static str {
        "prometheus-exporter"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn init(_config: alumet::plugin::ConfigTable) -> anyhow::Result<Box<Self>> {
        Ok(Box::new(PrometheusPlugin {}))
    }

    fn start(&mut self, alumet: &mut alumet::plugin::AlumetStart) -> anyhow::Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], 8001));

        alumet.add_output_builder(move |pipeline| {
            // Get the tokio's runtime used by Alumet for this output
            // and start the HTTP server on it.
            let rt = pipeline.async_runtime_handle();
            rt.spawn(output::run_http_server(addr));
            
            Ok(Box::new(output::PrometheusOutput {}))
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // TODO make sure that the http server stops
        Ok(())
    }
}
