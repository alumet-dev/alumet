use std::net::SocketAddr;

use alumet::{
    pipeline::elements::source::builder::AutonomousSourceRegistration,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable,
    },
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tonic::transport::Server;

use crate::{resolve_socket_address, server::grpc};

pub struct RelayServerPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Address to listen on.
    /// The default value is ip6-localhost = `::1`.
    ///
    /// To listen all your network interfaces please use `0.0.0.0` or `::`.
    address: String,

    /// Port on which to serve.
    port: u16,

    /// IPv6 scope id, for link-local addressing.
    ipv6_scope_id: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: String::from("::1"), // "any" on ipv6
            port: 50051,
            ipv6_scope_id: None,
        }
    }
}

impl AlumetPlugin for RelayServerPlugin {
    fn name() -> &'static str {
        "relay-server"
    }

    fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn default_config() -> anyhow::Result<Option<ConfigTable>> {
        let config = serialize_config(Config::default())?;
        Ok(Some(config))
    }

    fn init(config: ConfigTable) -> anyhow::Result<Box<Self>> {
        let config: Config = deserialize_config(config)?;

        Ok(Box::new(RelayServerPlugin { config }))
    }

    fn start(&mut self, alumet: &mut AlumetPluginStart) -> anyhow::Result<()> {
        // Resolve the address from the config.
        let config_address = std::mem::take(&mut self.config.address);
        let addr: SocketAddr = resolve_socket_address(config_address, self.config.port, self.config.ipv6_scope_id)?[0];

        log::info!("Starting gRPC server with on socket {addr}");
        alumet.add_autonomous_source_builder(move |ctx, cancel_token, out_tx| {
            let metrics_tx = ctx.metrics_sender();
            let collector = grpc::MetricCollectorImpl::new(out_tx, metrics_tx);
            let source = Box::pin(async move {
                Server::builder()
                    .add_service(collector.into_service())
                    .serve_with_shutdown(addr, cancel_token.cancelled_owned())
                    .await
                    .context("gRPC server error")?;
                Ok(())
            });
            Ok(AutonomousSourceRegistration {
                name: ctx.source_name("grpc-server"),
                source,
            })
        });
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // The autonomous source has already been stopped at this point.
        Ok(())
    }
}
