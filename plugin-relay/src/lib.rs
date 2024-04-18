#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

pub mod protocol {
    tonic::include_proto!("alumet_relay");   
}
