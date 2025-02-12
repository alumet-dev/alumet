use std::net::ToSocketAddrs;

use alumet::plugin::{
    rust::{deserialize_config, serialize_config, AlumetPlugin},
    AlumetPluginStart, ConfigTable,
};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::server::source;

pub struct RelayServerPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Address to listen on.
    /// The default value is "IPv6 any" on port 50051, i.e. `[::]:50051`.
    ///
    /// For information, ip6-localhost is `::1`.
    /// To listen to all your network interfaces please use `0.0.0.0` or `::`.
    address: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: String::from("[::]:50051"), // "any" on ipv6
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
        // Resolve the address from the config right now (fail fast).
        let addr = std::mem::take(&mut self.config.address);
        let addr: Vec<_> = addr
            .to_socket_addrs()
            .with_context(|| format!("invalid socket address: {addr}"))?
            .collect();

        // Register the source builder.
        alumet.add_autonomous_source_builder("tcp_server", move |ctx, cancel_token, out_tx| {
            log::info!("Starting relay server on: {addr:?}");
            let metrics_tx = ctx.metrics_sender();
            let source = Box::pin(async move {
                // `bind` loops through all the addresses that correspond to the string
                let listener = TcpListener::bind(addr.as_slice()).await.context("tcp binding failed")?;
                let server = source::TcpServer::new(cancel_token, listener, out_tx, metrics_tx);
                server.accept_loop().await
            });
            Ok(source)
        })?;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        // The autonomous source has already been stopped at this point.
        Ok(())
    }
}
