use std::net::SocketAddr;

use alumet::{
    pipeline::elements::source::builder::AutonomousSourceRegistration,
    plugin::{
        rust::{deserialize_config, serialize_config, AlumetPlugin},
        AlumetPluginStart, ConfigTable,
    },
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::{server::source, util::resolve_socket_address};

pub struct RelayServerPlugin {
    config: Config,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Address to listen on.
    /// The default value is "IPv6 any", i.e. `::`.
    ///
    /// For information, ip6-localhost is `::1`.
    ///
    /// To listen to all your network interfaces please use `0.0.0.0` or `::`.
    address: String,

    /// Port on which to serve.
    port: u16,

    /// IPv6 scope id, for link-local addressing.
    ipv6_scope_id: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            address: String::from("::"), // "any" on ipv6
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

        alumet.add_autonomous_source_builder(move |ctx, cancel_token, out_tx| {
            log::info!("Starting relay server on socket {addr}");
            let metrics_tx = ctx.metrics_sender();
            let source = Box::pin(async move {
                let listener = TcpListener::bind(addr).await?;
                let server = source::TcpServer::new(cancel_token, listener, out_tx, metrics_tx);
                server.accept_loop().await
            });
            Ok(AutonomousSourceRegistration {
                name: ctx.source_name("relay-server"),
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
