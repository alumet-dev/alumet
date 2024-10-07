use std::{
    collections::HashSet,
    net::{SocketAddr, ToSocketAddrs},
};

use anyhow::{anyhow, Context};

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

pub(crate) mod protocol {
    tonic::include_proto!("alumet_relay");
}

pub const CLIENT_NAME_HEADER: &str = "x-alumet-client";

/// Parses and resolves a socket address, made of an `address` and a `port`.
///
/// # Result
/// Returns a **non-empty** vector of socket addresses, or an error.
pub fn resolve_socket_address(
    address: String,
    port: u16,
    ipv6_scope_id: Option<u32>,
) -> anyhow::Result<Vec<SocketAddr>> {
    // Resolve the address and port. This may return multiple results.
    let socket_addrs: Vec<SocketAddr> = (address.clone(), port)
        .to_socket_addrs()
        .with_context(|| format!("invalid address: {address}"))?
        .collect();

    fn has_correct_scope(addr: &SocketAddr, ipv6_scope_id: Option<u32>) -> bool {
        match (addr, ipv6_scope_id) {
            (SocketAddr::V6(addr), Some(scope_id)) => addr.scope_id() == scope_id,
            (SocketAddr::V4(_), Some(_scope_id)) => false,
            _ => true,
        }
    }

    match socket_addrs[..] {
        [] => Err(anyhow!("no address found when resolving ({address}, {port})")),
        [single] => {
            // Set the IPv6 scope id if applicable.
            match single {
                SocketAddr::V4(addr4) => Ok(vec![std::net::SocketAddr::V4(addr4)]),
                SocketAddr::V6(mut addr6) => {
                    if let Some(scope_id) = ipv6_scope_id {
                        addr6.set_scope_id(scope_id);
                    }
                    Ok(vec![std::net::SocketAddr::V6(addr6)])
                }
            }
        }
        _ => {
            // filter
            let mut addresses: Vec<_> = socket_addrs
                .into_iter()
                .filter(|addr| has_correct_scope(addr, ipv6_scope_id))
                .collect();
            // deduplicate but preserve order (the addresses should be tried in the order returned by the system, see man getaddrinfo)
            let mut seen: HashSet<SocketAddr> = HashSet::with_capacity(addresses.len());
            addresses.retain(|addr| seen.insert(*addr));
            if addresses.is_empty() {
                Err(anyhow!(
                    "no address matches ({address}, {port}) and ipv6 scope id {}",
                    ipv6_scope_id.unwrap()
                ))
            } else {
                Ok(addresses)
            }
        }
    }
}
